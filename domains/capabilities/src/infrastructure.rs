//! Butler **capabilities** (skills) — the modules the butler can reach for
//! (ADR 0056 §capabilities). Each is a self-contained unit that declares what it
//! does, its **autonomy level** (may it act, or must it ask?), whether it
//! **reaches outside the machine**, and whether it is **configured** (ready, or
//! waiting on a key / model / data source). Consequential or unconfigured skills
//! are surfaced but gated by the policy layer — the butler proposes, the person
//! authorizes; the model is never the enforcement boundary.
//!
//! MCP note: these are the internal `Capability` interface; an MCP server is one
//! way to back a capability (ADR 0056 §3). The registry here is the substrate a
//! future MCP host adapter plugs into.

use std::sync::Arc;
use std::time::Duration;

use crate::application::{CapabilityRunner, Stance};
use endora_kernel::{Decision, Reversibility};
use serde_json::{Value, json};

/// One setting a capability needs to work (a key, a model name, a URL). Declared
/// in metadata so the console can render a form and the policy layer can tell
/// whether the skill is ready (ADR 0054).
#[derive(Debug, Clone, Copy)]
pub struct SettingSpec {
    /// Stable key the value is stored under, e.g. `"model"` or `"api_key"`.
    pub key: &'static str,
    /// Human label for the form field.
    pub label: &'static str,
    /// Whether the value is a secret (never echoed back to the client).
    pub secret: bool,
    /// Whether the skill works fine without it.
    ///
    /// Every setting used to be required, which quietly meant a skill with an optional one
    /// could never be "configured". Home Assistant's `mcp_server` is documented as *blank =
    /// home-assistant* and `notify_service` as *blank = never* — both are meant to be left
    /// empty, and leaving them empty is what kept the whole skill reading **needs setup**
    /// and out of the model's catalogue entirely.
    ///
    /// A field the interface itself describes as optional cannot also be the reason a skill
    /// is switched off.
    pub optional: bool,
}

/// A capability's stored settings, keyed by [`SettingSpec::key`]. Passed to
/// `invoke` so a skill reads its configuration (model, key, …) at run time.
pub type CapabilitySettings = std::collections::BTreeMap<String, String>;

/// Static metadata describing a capability to the person and the policy layer.
#[derive(Debug, Clone)]
pub struct CapabilityInfo {
    /// Stable slug, e.g. `"weather"`.
    pub id: &'static str,
    /// Human name, e.g. `"Weather"`.
    pub name: &'static str,
    /// One-line description of what it does.
    pub description: &'static str,
    /// Grouping for the UI: `information`, `safety`, `travel`, `media`, `presence`.
    pub category: &'static str,
    /// Whether invoking it sends data outside this machine.
    pub reaches_external: bool,
    /// How undoable the skill's effect is — the **primary axis** of the autonomy
    /// envelope (ADR 0051). Declared in metadata, never inferred by a model: it
    /// decides whether policy may run the skill on its own, confirm first, or
    /// block it outright. The classifier NEVER runs an irreversible skill
    /// (deny-by-default).
    pub reversibility: Reversibility,
    /// Whether the code is ready in principle (ignoring settings). Effective
    /// readiness also requires every [`Self::settings`] to have a value.
    pub configured: bool,
    /// If not configured, a short note on what it needs.
    pub needs: &'static str,
    /// The settings this capability needs to run (empty for keyless skills). A
    /// skill is usable only once all of these have values.
    pub settings: &'static [SettingSpec],
    /// The JSON-Schema for this skill's arguments, when it takes any.
    ///
    /// Built-in skills had no way to say. Every one of them was offered to the model with an
    /// empty parameter object — *this takes nothing* — and the two that really do take
    /// arguments got by on a hand-written example inside the system prompt, which is a
    /// per-skill patch and only ever covered the skills somebody remembered.
    ///
    /// A new skill that needed a venue was therefore called with `{}`, twice, and answered
    /// *"say what to look for"* both times. It was doing exactly what it was told it could.
    ///
    /// MCP tools have carried a real schema all along; this is the same thing for the skills
    /// written here, so the model is told the field names rather than left to guess them.
    /// `None` for a skill that genuinely takes nothing.
    pub input_schema: Option<&'static str>,
}

/// Why a capability call failed.
#[derive(Debug, Clone)]
pub enum CapabilityError {
    /// The capability is not set up (missing key/model/source) or unreachable.
    Unavailable(String),
    /// The input was missing or malformed.
    BadInput(String),
}

impl std::fmt::Display for CapabilityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable(m) => write!(f, "unavailable: {m}"),
            Self::BadInput(m) => write!(f, "bad input: {m}"),
        }
    }
}

/// A butler skill: metadata plus a synchronous `invoke` (run on a blocking
/// worker in the node, like the model calls).
pub trait Capability: Send + Sync {
    /// Static description of this capability.
    fn info(&self) -> CapabilityInfo;

    /// Runs the capability with JSON `input` and its configured `settings`,
    /// returning a JSON result. Keyless skills ignore `settings`.
    ///
    /// # Errors
    /// [`CapabilityError`] if the input is bad or the capability is unavailable.
    fn invoke(
        &self,
        input: &Value,
        settings: &CapabilitySettings,
    ) -> Result<Value, CapabilityError>;

    /// Hands back a [`NativeChannel`] onto the service this skill speaks for, if it has one.
    ///
    /// **This is the seam a local integration plugs into.** Some things Endora needs from a
    /// service have nowhere to live in a tool call — reading the whole world for the watch
    /// loop, presence for every turn, a setup form, writing config back and undoing it. A
    /// capability that can supply those returns a channel here; everything else keeps the
    /// default and is unaffected.
    ///
    /// Written because the node was assembling the one existing channel **by name**, in the
    /// composition root, with an early return that meant a second integration could never
    /// have been reached at all. Registration now runs off the same list every other skill is
    /// declared in ([`default_capabilities`]), so adding a local integration is the line you
    /// were already adding and nothing else.
    ///
    /// `aliases` is every confirmed name-for-a-thing Endora holds, across all servers. It is
    /// handed over whole and **the integration filters it**, because which of them belong to
    /// it is knowledge only it has — the alternative puts a per-integration branch back in
    /// shared code, which is the thing ADR 0054 exists to stop.
    ///
    /// Returning `None` is the ordinary case, and covers both "this skill has no service to
    /// watch" and "it has one but is not configured yet".
    fn channel(
        &self,
        _settings: &CapabilitySettings,
        _aliases: &[crate::domain::TargetAlias],
    ) -> Option<(String, std::sync::Arc<dyn NativeChannel>)> {
        None
    }

    /// Proves this skill actually works, right now, with the settings it has been given.
    ///
    /// Both model endpoints have had a **Test connection** button since they existed; no
    /// skill has ever had one. So the way to find out whether a URL, a token or a newly
    /// nominated notify service is right has been to configure it and wait — and a
    /// setting whose only feedback is a proactive message that may not arrive for hours is
    /// a setting nobody can be confident in.
    ///
    /// The default is not a stub: **a read-only skill proves itself by running.** It is
    /// invoked with no arguments and its own summary is returned, so every observing skill
    /// gets a working test without writing one. A skill that can actuate refuses, because
    /// "press this to find out" must never be how someone discovers what it does.
    ///
    /// Override it where a skill can prove *more* than that — a channel that can also
    /// reach the person should show that it can.
    ///
    /// # Errors
    /// [`CapabilityError`] if the skill cannot reach what it needs, or has no safe test.
    fn self_test(&self, settings: &CapabilitySettings) -> Result<String, CapabilityError> {
        if self.info().reversibility != Reversibility::Observe {
            return Err(CapabilityError::Unavailable(
                "this skill can change things, so there is no safe way to try it for you"
                    .to_owned(),
            ));
        }
        let out = self.invoke(&Value::Object(serde_json::Map::new()), settings)?;
        Ok(self.summarize(&out))
    }

    /// Renders a result into short, human-readable text for the butler to answer
    /// from. Small local models relay a clean sentence far better than raw JSON,
    /// so each skill that the butler speaks from overrides this. The default is
    /// the JSON itself (fine for programmatic consumers / the Skills UI).
    fn summarize(&self, output: &Value) -> String {
        output.to_string()
    }
}

/// Every [`NativeChannel`] the given skills can supply, with the server each speaks for.
///
/// The registration seam for local integrations. Each skill is asked once; almost all say no,
/// and the ones that say yes are the services Endora needs to *watch* rather than merely call.
///
/// **No integration is named here, deliberately.** The node used to assemble the one existing
/// channel by hand, reaching for Home Assistant's settings and returning an empty list when
/// they were absent — which meant a second local integration could not have been reached even
/// after somebody wrote it. Adding one is now a line in [`default_capabilities`] and nothing
/// else.
///
/// A skill with no settings is still asked, with an empty set, so "not configured" is the
/// integration's own judgement rather than a guess made out here about which keys matter.
#[must_use]
pub fn channels_of(
    capabilities: &[Arc<dyn Capability>],
    settings: &std::collections::HashMap<String, CapabilitySettings>,
    aliases: &[crate::domain::TargetAlias],
) -> Vec<(String, Arc<dyn NativeChannel>)> {
    let empty = CapabilitySettings::default();
    capabilities
        .iter()
        .filter_map(|capability| {
            let id = capability.info().id;
            capability.channel(settings.get(id).unwrap_or(&empty), aliases)
        })
        .collect()
}

/// The smallest arguments that will make a tool actually do its work, from its own schema.
///
/// A test that proves nothing is worse than no test. Calling a tool with `{}` gets a
/// validation error back from the server without a single byte leaving for the service
/// behind it — which looks like a failure while saying nothing at all about the credential,
/// the thing anybody presses Test to find out about.
///
/// Live: a Brave server listed eight tools, connected cleanly, reported no error, and was
/// subscribed to the wrong API. Everything on the card looked healthy because a handshake
/// never calls the service. Only a real call can tell you.
///
/// So: every **required** property gets the blandest value of its declared type, and nothing
/// optional is invented. An `enum` yields its first member rather than a guess, because a
/// made-up string is the one thing certain to be rejected locally.
///
/// From the schema the server published, never from a table of server names — the same rule
/// that keeps every other integration from needing its own branch here.
#[must_use]
pub fn arguments_for_a_test_call(schema: Option<&str>) -> String {
    let Some(parsed) = schema.and_then(|s| serde_json::from_str::<Value>(s).ok()) else {
        return "{}".to_owned();
    };
    let required: Vec<&str> = parsed
        .get("required")
        .and_then(Value::as_array)
        .map(|r| r.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let mut args = serde_json::Map::new();
    for key in required {
        let Some(field) = parsed.get("properties").and_then(|p| p.get(key)) else {
            continue;
        };
        // A declared choice beats anything invented: the server already told us what it
        // will accept.
        if let Some(first) = field
            .get("enum")
            .and_then(Value::as_array)
            .and_then(|c| c.first())
        {
            args.insert(key.to_owned(), first.clone());
            continue;
        }
        let value = match field.get("type").and_then(Value::as_str) {
            Some("string") => json!("test"),
            Some("integer" | "number") => json!(1),
            Some("boolean") => json!(false),
            Some("array") => json!([]),
            Some("object") => json!({}),
            // A required field of unstated type is left out rather than guessed. The server
            // will say what it wanted, which is more use than a rejected invention.
            _ => continue,
        };
        args.insert(key.to_owned(), value);
    }
    Value::Object(args).to_string()
}

/// Which tools the auto-allow toggle governs: **every one the server exposes**, whichever
/// way it is being moved.
///
/// The distinction this draws against [`tools_to_open_on_connect`] is the whole of it, and
/// getting it wrong left a server reading *"Allow all its tools: On"* with all eight of them
/// blocked:
///
/// - **Connect is a default.** It runs at every start-up, nobody asked for it this time, and
///   it must never overwrite a decision — that rule was written after a standing flag
///   silently restored a capability somebody had said no to.
/// - **The toggle is a decision**, made just now, by the person, about this server. A
///   decision is exactly the thing that *may* overwrite an older one.
///
/// Only the off direction was implemented, and it was right to close everything: nothing
/// records which tools auto-allow opened, so closing them all is the safe direction. But on
/// did nothing except store the flag, leaving the opening to connect — which correctly skips
/// every tool anybody has ruled on, and after an off that is all of them.
///
/// So off-then-on was a one-way door in the opposite direction to the one that fix closed,
/// and the way back was opening each tool by hand.
#[must_use]
pub fn tools_the_toggle_governs(available: &[String], server: &str) -> Vec<String> {
    let prefix = format!("{server}.");
    available
        .iter()
        .filter(|id| id.starts_with(&prefix))
        .cloned()
        .collect()
}

/// Which of a trusted server's tools should be opened on connect.
///
/// `trust_all` means **"allow the tools I have not decided about"**, not "re-open everything,
/// forever". A tool the person has already ruled on — either way — is left exactly as they
/// left it.
///
/// Written after a standing flag quietly undid an explicit instruction. Voice broadcast was
/// blocked at the person's request, and came back on: connect runs at every start-up and
/// re-opened every tool on the trusted server unconditionally, so four deploys in an afternoon
/// silently restored a capability they had said no to. Nothing announced it, because from the
/// system's point of view nothing had gone wrong.
///
/// That is [0054](../../docs/adr/0054-other-peoples-services.md)'s own rule being broken from
/// the inside: **confirmed beats declared**. A person's decision is confirmed; `trust_all` is a
/// standing default, and a default that overwrites a decision is not a default.
///
/// New tools appearing on a trusted server are still opened, which is the whole point of the
/// flag — they have no decision to overwrite.
#[must_use]
pub fn tools_to_open_on_connect(
    available: &[String],
    trusted_prefixes: &[String],
    already_decided: &[String],
) -> Vec<String> {
    available
        .iter()
        .filter(|id| trusted_prefixes.iter().any(|p| id.starts_with(p)))
        .filter(|id| !already_decided.iter().any(|d| d == *id))
        .cloned()
        .collect()
}

/// Builds the default set of capabilities the node offers. Read-only information
/// skills are ready; the rest are declared but await configuration, so they show
/// up as modules to enable rather than silently missing.
#[must_use]
pub fn default_capabilities() -> Vec<Arc<dyn Capability>> {
    vec![
        Arc::new(WeatherCapability),
        Arc::new(WebFetchCapability),
        Arc::new(KnowledgeCapability),
        Arc::new(LocalNewsCapability),
        Arc::new(ImageReviewCapability::from_env()),
        Arc::new(CityMeetingsCapability),
        Arc::new(TicketedEventsCapability),
        Arc::new(FlightSearchCapability),
        Arc::new(LocationLogCapability),
        Arc::new(SafetyAlertsCapability),
        Arc::new(IncidentScannerCapability),
        Arc::new(HomeAssistantCapability),
    ]
}

// ---- helpers ---------------------------------------------------------------

/// Reads a service address the person typed, and makes it a URL.
///
/// `192.168.1.10:8123` is what someone types when asked for their Home Assistant address,
/// and it is not a URL — every request built from it fails with `http: invalid format`.
///
/// This was live and invisible. Endora's **direct reach** into the house had been dead:
/// presence, live states, the standing-trouble watch and the facts behind an answer all
/// silently returned nothing, while the MCP tools kept working because that server talks
/// to Home Assistant on its own. Nothing looked broken, because a channel that cannot
/// answer is indistinguishable from a service with nothing to say — the same trap
/// [0054](../../docs/adr/0054-other-peoples-services.md) records about defaulted port
/// methods, arriving this time through a text box.
///
/// **`http` rather than `https`** when the scheme is missing: these are addresses on the
/// person's own network, typed as a bare host and port, and that is what such a service
/// is almost always serving. Someone who needs TLS has a hostname and will type the
/// scheme.
pub(crate) fn as_url(typed: &str) -> String {
    let address = typed.trim().trim_end_matches('/');
    if address.is_empty() || address.contains("://") {
        return address.to_owned();
    }
    format!("http://{address}")
}

/// A **direct** HTTP agent — no egress proxy. Used for trusted internal calls (the
/// local vision model), which must never be routed through a VPN/proxy.
pub(crate) fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(15)))
        .build()
        .into()
}

/// The configured egress proxy, if any. `ENDORA_EGRESS_PROXY` is an `http://`,
/// `https://`, or `socks5://` URL (trusted deployment config). See
/// [`external_agent`].
fn egress_proxy() -> Option<ureq::Proxy> {
    let url = std::env::var("ENDORA_EGRESS_PROXY").ok()?;
    let url = url.trim();
    if url.is_empty() {
        return None;
    }
    ureq::Proxy::new(url).ok()
}

/// An agent for **external** skill egress. If an egress proxy is configured,
/// outbound skill requests route through it — so you can send them via a VPN's
/// proxy (e.g. gluetun's HTTP proxy) **without** binding Endora's network to the VPN
/// container (ADR 0051). Loosely coupled: if the proxy is down, only external skills
/// fail; the app keeps running.
fn external_agent() -> ureq::Agent {
    let mut builder = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(15)));
    if let Some(proxy) = egress_proxy() {
        builder = builder.proxy(Some(proxy));
    }
    builder.build().into()
}

// ---- Answers worth keeping (ADR 0061) --------------------------------------

/// How long an answer is kept when the source says nothing, and the shortest it is ever
/// kept even when the source says less.
///
/// A source is not owed unlimited trust about its own freshness. Something claiming
/// `max-age=0` would make this pointless; the floor decides instead.
const KEEP_AT_LEAST_MS: u64 = 60_000;

/// The longest an answer is kept, whatever the source claims.
///
/// A ceiling because a source claiming a year would otherwise make the butler wrong for a
/// year, and being confidently out of date is worse than being slow.
const KEEP_AT_MOST_MS: u64 = 6 * 60 * 60 * 1_000;

/// How many answers are remembered at once. Oldest goes first.
const MOST_ANSWERS_KEPT: usize = 256;

/// One remembered answer.
struct Remembered {
    body: String,
    /// When it stops being served without asking again.
    fresh_until_ms: u64,
    /// When it arrived — used to decide what to forget first, and nothing else.
    stored_ms: u64,
}

/// Everything remembered, in memory only (ADR 0061).
///
/// Never written to disk. A cache keyed by what somebody asked is a **record of questions**,
/// and persisting it would create an obligation to purge it on *forget everything* that a
/// warm start does not justify.
static REMEMBERED: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<(u64, u64), Remembered>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// A transport failure, said the way a person would say it.
///
/// Shown live, as the whole of an answer:
///
/// ```text
/// error: unavailable: io: invalid peer certificate: certificate expired: verification
/// time 1785717318 (UNIX), but certificate is not valid after 1543938436 (241778882
/// seconds ago)
/// ```
///
/// Every word of that is true and none of it is an answer. The person asked a question; a
/// site they have never heard of has a certificate that lapsed in 2018, and there is nothing
/// they can do about it. **What matters is that the page could not be read and why in one
/// clause** — the rest is plumbing, and the butler does not narrate plumbing.
///
/// Mapped on what the message contains rather than on a status code, because these arrive
/// as prose from a transport that owes us no shape. Anything unrecognised is passed through:
/// a wrong guess would be worse than a technical sentence, and unrecognised is where the
/// next unknown failure lives.
#[must_use]
pub fn plainly(said: &str) -> String {
    let lower = said.to_ascii_lowercase();
    let plain = if lower.contains("certificate expired") || lower.contains("certificatexpired") {
        "that site's security certificate has expired, so I would not trust the page"
    } else if lower.contains("certificate") {
        "that site's security certificate did not check out, so I would not trust the page"
    } else if lower.contains("timed out") || lower.contains("timeout") {
        "that site took too long to answer"
    } else if lower.contains("dns") || lower.contains("resolve") {
        "I could not find that site at all"
    } else if lower.contains("connection refused") || lower.contains("connect") {
        "that site would not let me connect"
    } else {
        return said.to_owned();
    };
    plain.to_owned()
}

/// A fingerprint of a request — enough to recognise it again, not enough to rebuild it.
///
/// **The URL is never stored.** It carries the API key of whatever is being asked, and often
/// the person's own town. The node holds those elsewhere and must; what this refuses to do
/// is create a *second* place they can be read from. There is nothing here to redact in a
/// log, because there is nothing here to print.
///
/// Two independent hashes rather than one, because a collision would not be a slow answer —
/// it would be another question's answer, delivered confidently.
fn fingerprint(url: &str) -> (u64, u64) {
    use std::hash::{Hash, Hasher};
    let once = {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        url.hash(&mut h);
        h.finish()
    };
    let twice = {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        // A different starting state, so the two are not the same function twice.
        "endora/0061".hash(&mut h);
        url.hash(&mut h);
        h.finish()
    };
    (once, twice)
}

/// How long the source says its answer is good for, within bounds.
///
/// `Cache-Control: max-age` decides it, per response, per source — which is the whole reason
/// there is no table of which source is hourly and which is monthly. Such a table would go
/// stale, and it is exactly the per-integration knowledge that belongs nowhere near shared
/// code.
#[must_use]
pub fn keep_for_ms(cache_control: Option<&str>) -> u64 {
    let Some(said) = cache_control else {
        return KEEP_AT_LEAST_MS;
    };
    let said = said.to_ascii_lowercase();
    // The one directive worth obeying exactly: the source is saying do not keep this at all.
    // `max-age=0` is NOT that — it is a claim about freshness, and a claim about freshness is
    // the thing the floor exists to bound.
    if said.contains("no-store") {
        return 0;
    }
    let value_of = |name: &str| {
        said.split(',').find_map(|part| {
            part.trim()
                .strip_prefix(name)?
                .trim()
                .parse::<u64>()
                .ok()
                .map(|seconds| seconds.saturating_mul(1000))
        })
    };
    // `s-maxage` first wherever both appear: it is the one addressed to a shared cache,
    // which is what this is. Order in the header must not decide it.
    value_of("s-maxage=")
        .or_else(|| value_of("max-age="))
        .map_or(KEEP_AT_LEAST_MS, |ms| {
            ms.clamp(KEEP_AT_LEAST_MS, KEEP_AT_MOST_MS)
        })
}

/// Forgets the oldest answers once there are too many.
fn forget_the_oldest(kept: &mut std::collections::HashMap<(u64, u64), Remembered>) {
    while kept.len() > MOST_ANSWERS_KEPT {
        let Some(oldest) = kept
            .iter()
            .min_by_key(|(_, r)| r.stored_ms)
            .map(|(k, _)| *k)
        else {
            return;
        };
        kept.remove(&oldest);
    }
}

/// GETs a URL, remembering the answer for as long as the source says it is good (ADR 0061).
///
/// One place, so a skill written the ordinary way is cached without opting in and a new one
/// does nothing to take part. The capability runner was the tempting layer and the wrong
/// one: by the time a result reaches it the origin's headers are gone, and recovering them
/// would mean every skill reporting its own freshness.
///
/// **A stale answer beats no answer.** If the source is down or the quota is spent and there
/// is an old answer, it is served — the failure that has cost most here is a screen quietly
/// saying it knows nothing when it does.
fn get_remembering(
    url: &str,
    ua: Option<&str>,
    max_bytes: usize,
) -> Result<String, CapabilityError> {
    use std::io::Read;
    let id = fingerprint(url);
    let now = now_ms();
    if let Ok(kept) = REMEMBERED.lock() {
        if let Some(found) = kept.get(&id) {
            if now < found.fresh_until_ms {
                return Ok(found.body.clone());
            }
        }
    }

    let mut request = external_agent().get(url);
    if let Some(ua) = ua {
        request = request.header("User-Agent", ua);
    }
    let asked = request.call().and_then(|mut resp| {
        let keep = keep_for_ms(
            resp.headers()
                .get("cache-control")
                .and_then(|v| v.to_str().ok()),
        );
        let mut buf = Vec::new();
        resp.body_mut()
            .as_reader()
            .take(max_bytes as u64)
            .read_to_end(&mut buf)?;
        Ok((String::from_utf8_lossy(&buf).into_owned(), keep))
    });

    match asked {
        Ok((body, keep)) => {
            if keep > 0 {
                if let Ok(mut kept) = REMEMBERED.lock() {
                    kept.insert(
                        id,
                        Remembered {
                            body: body.clone(),
                            fresh_until_ms: now.saturating_add(keep),
                            stored_ms: now,
                        },
                    );
                    forget_the_oldest(&mut kept);
                }
            }
            Ok(body)
        }
        Err(e) => {
            // Stale rather than nothing.
            if let Ok(kept) = REMEMBERED.lock() {
                if let Some(found) = kept.get(&id) {
                    return Ok(found.body.clone());
                }
            }
            Err(CapabilityError::Unavailable(plainly(&e.to_string())))
        }
    }
}

/// GETs a URL and returns the body as text (size-capped), for the info skills.
fn http_get_text(url: &str, max_bytes: usize) -> Result<String, CapabilityError> {
    get_remembering(url, None, max_bytes)
}

fn str_field<'a>(input: &'a Value, key: &str) -> Result<&'a str, CapabilityError> {
    input
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| CapabilityError::BadInput(format!("missing '{key}'")))
}

/// GET with an explicit `User-Agent` (some APIs, e.g. the US NWS, require one).
fn http_get_text_ua(url: &str, ua: &str, max_bytes: usize) -> Result<String, CapabilityError> {
    get_remembering(url, Some(ua), max_bytes)
}

// ---- Egress guard (SSRF protection for model/person-provided URLs) ----------

/// Rejects a URL whose host is, or resolves to, a non-public address — closing the
/// SSRF hole where a model-provided URL could reach the internal network or a cloud
/// metadata endpoint (ADR 0051). Only for **arbitrary** URLs; the trusted internal
/// model calls and constant-host API skills do not use this.
fn guard_egress(url: &str) -> Result<(), CapabilityError> {
    let (host, port) = host_and_port(url)
        .ok_or_else(|| CapabilityError::BadInput(format!("couldn't parse URL host: {url}")))?;
    let deny = |ip: &std::net::IpAddr| {
        // Anything that isn't a normal public address is refused.
        match ip {
            std::net::IpAddr::V4(v4) => {
                v4.is_loopback()
                    || v4.is_private()
                    || v4.is_link_local()
                    || v4.is_unspecified()
                    || v4.is_broadcast()
                    || v4.is_multicast()
                    || v4.is_documentation()
                    // Carrier-grade NAT 100.64.0.0/10.
                    || (v4.octets()[0] == 100 && (v4.octets()[1] & 0xc0) == 0x40)
            }
            std::net::IpAddr::V6(v6) => {
                v6.is_loopback()
                    || v6.is_unspecified()
                    || v6.is_multicast()
                    // Unique-local fc00::/7 and link-local fe80::/10.
                    || (v6.segments()[0] & 0xfe00) == 0xfc00
                    || (v6.segments()[0] & 0xffc0) == 0xfe80
            }
        }
    };
    let blocked = |ip: std::net::IpAddr| {
        if deny(&ip) {
            Err(CapabilityError::BadInput(format!(
                "refusing to fetch a private/internal address ({ip})"
            )))
        } else {
            Ok(())
        }
    };
    // A literal IP is checked directly; a hostname is resolved first (so a name that
    // points at an internal IP is caught too).
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        return blocked(ip);
    }
    use std::net::ToSocketAddrs;
    let addrs = (host.as_str(), port)
        .to_socket_addrs()
        .map_err(|e| CapabilityError::Unavailable(format!("couldn't resolve {host}: {e}")))?;
    let mut any = false;
    for a in addrs {
        any = true;
        blocked(a.ip())?;
    }
    if any {
        Ok(())
    } else {
        Err(CapabilityError::Unavailable(format!(
            "couldn't resolve {host}"
        )))
    }
}

/// Extracts `(host, port)` from an http(s) URL, without a URL dependency. Strips
/// userinfo, handles IPv6 literals `[..]`, and defaults the port by scheme.
fn host_and_port(url: &str) -> Option<(String, u16)> {
    let scheme_end = url.find("://")?;
    let scheme = &url[..scheme_end];
    let default_port = match scheme {
        "http" => 80,
        "https" => 443,
        _ => return None,
    };
    let after = &url[scheme_end + 3..];
    let authority = after.split(['/', '?', '#']).next()?;
    // Strip any userinfo ("user:pass@host").
    let authority = authority.rsplit('@').next()?;
    if let Some(rest) = authority.strip_prefix('[') {
        // IPv6 literal: [addr] or [addr]:port.
        let end = rest.find(']')?;
        let host = rest[..end].to_owned();
        let port = rest[end + 1..]
            .strip_prefix(':')
            .and_then(|p| p.parse().ok())
            .unwrap_or(default_port);
        return Some((host, port));
    }
    match authority.rsplit_once(':') {
        Some((host, port)) if !host.is_empty() => {
            Some((host.to_owned(), port.parse().unwrap_or(default_port)))
        }
        _ => Some((authority.to_owned(), default_port)),
    }
}

/// The data-loss tripwire (ADR 0051): scans text about to leave the machine (an
/// external skill's input) for **high-confidence secrets**, so the butler can't be
/// steered into leaking a key or private key in a query. Deliberately precise —
/// only well-known credential shapes — to avoid blocking legitimate requests.
/// Returns a short label of what was found, or `None` if the text looks clean.
pub fn scan_outbound_secret(text: &str) -> Option<&'static str> {
    if text.contains("PRIVATE KEY-----") {
        return Some("a private key");
    }
    // Split on whitespace, JSON/markup punctuation, AND URL delimiters, so a secret
    // embedded in a query string (…?token=sk-…) is isolated as its own token.
    text.split(|c: char| {
        c.is_whitespace()
            || matches!(
                c,
                '"' | '\''
                    | '<'
                    | '>'
                    | '('
                    | ')'
                    | '{'
                    | '}'
                    | '['
                    | ']'
                    | ','
                    | ';'
                    | '\\'
                    | '='
                    | '?'
                    | '&'
                    | '/'
                    | ':'
                    | '#'
                    | '@'
                    | '|'
            )
    })
    .find_map(classify_secret_token)
}

/// Query minimization (ADR 0051): redacts personal identifiers from an external
/// skill's input before it leaves — so a search doesn't carry a real email address
/// out. Recurses through the JSON, redacting string values in place. Deliberately
/// narrow (email addresses) and word-boundaried, so URLs and ordinary text survive.
pub fn redact_pii_in_value(v: &mut Value) {
    match v {
        Value::String(s) => *s = redact_emails_in_text(s),
        Value::Array(a) => a.iter_mut().for_each(redact_pii_in_value),
        Value::Object(o) => o.values_mut().for_each(redact_pii_in_value),
        _ => {}
    }
}

/// Replaces whole-word email addresses in free text with `[redacted-email]`. Splits
/// on spaces so a bare URL (one word, not an email) is never altered — only a
/// standalone address in a query is caught.
fn redact_emails_in_text(text: &str) -> String {
    text.split(' ')
        .map(|word| {
            let core = word.trim_matches(|c: char| {
                !(c.is_ascii_alphanumeric() || matches!(c, '@' | '.' | '_' | '-' | '+'))
            });
            if !core.is_empty() && looks_like_email(core) {
                word.replace(core, "[redacted-email]")
            } else {
                word.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Whether a token is a bare email address (and not a URL).
fn looks_like_email(s: &str) -> bool {
    if s.contains('/') || s.contains(':') {
        return false;
    }
    let mut parts = s.split('@');
    let (Some(local), Some(domain), None) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    !local.is_empty()
        && domain.contains('.')
        && domain
            .rsplit('.')
            .next()
            .is_some_and(|tld| tld.len() >= 2 && tld.chars().all(|c| c.is_ascii_alphabetic()))
}

/// Classifies a single token as a known credential shape, or `None`.
fn classify_secret_token(t: &str) -> Option<&'static str> {
    let n = t.len();
    // Tail after a prefix is credential-like (alnum plus `_`/`-`).
    let tail_ok = |prefix: &str, min: usize| {
        t.len() >= min
            && t.starts_with(prefix)
            && t[prefix.len()..]
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    };
    // AWS access key id: AKIA + 16 uppercase/digits.
    if n == 20
        && t.starts_with("AKIA")
        && t[4..]
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
    {
        return Some("an AWS access key");
    }
    if tail_ok("sk-ant-", 24) || tail_ok("sk-", 20) {
        return Some("an API key");
    }
    if tail_ok("github_pat_", 22)
        || tail_ok("ghp_", 20)
        || tail_ok("gho_", 20)
        || tail_ok("ghs_", 20)
    {
        return Some("a GitHub token");
    }
    if (t.starts_with("xoxb-") || t.starts_with("xoxp-") || t.starts_with("xoxa-")) && n >= 24 {
        return Some("a Slack token");
    }
    if (t.starts_with("sk_live_") || t.starts_with("rk_live_")) && n >= 20 {
        return Some("a Stripe key");
    }
    if n == 39 && t.starts_with("AIza") {
        return Some("a Google API key");
    }
    // JWT: three base64url segments separated by dots.
    if t.starts_with("eyJ") {
        let parts: Vec<&str> = t.split('.').collect();
        let is_b64url = |c: char| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '=';
        if parts.len() == 3
            && parts
                .iter()
                .all(|p| p.len() >= 8 && p.chars().all(is_b64url))
        {
            return Some("a token (JWT)");
        }
    }
    None
}

/// Like [`http_get_text`], but guards the URL against SSRF and follows redirects
/// manually, re-guarding each hop (ADR 0051). For model/person-provided URLs.
fn guarded_get_text(url: &str, max_bytes: usize) -> Result<String, CapabilityError> {
    let bytes = guarded_get_bytes(url, max_bytes)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// Guarded byte fetch with manual, re-guarded redirect following (bounded).
fn guarded_get_bytes(url: &str, max_bytes: usize) -> Result<Vec<u8>, CapabilityError> {
    use std::io::Read;
    // No auto-redirects (we re-guard each hop), and honour the egress proxy.
    let mut builder = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(15)))
        .max_redirects(0);
    if let Some(proxy) = egress_proxy() {
        builder = builder.proxy(Some(proxy));
    }
    let no_redirect: ureq::Agent = builder.build().into();
    let mut current = url.to_owned();
    for _ in 0..6 {
        guard_egress(&current)?;
        let mut resp = no_redirect
            .get(&current)
            .header("User-Agent", "Mozilla/5.0 (Endora personal butler)")
            .call()
            .map_err(|e| CapabilityError::Unavailable(e.to_string()))?;
        let status = resp.status().as_u16();
        if (300..400).contains(&status) {
            let location = resp
                .headers()
                .get("location")
                .and_then(|v| v.to_str().ok())
                .ok_or_else(|| {
                    CapabilityError::Unavailable("redirect without location".to_owned())
                })?
                .to_owned();
            current = resolve_redirect(&current, &location);
            continue;
        }
        let mut buf = Vec::new();
        resp.body_mut()
            .as_reader()
            .take(max_bytes as u64)
            .read_to_end(&mut buf)
            .map_err(|e| CapabilityError::Unavailable(e.to_string()))?;
        return Ok(buf);
    }
    Err(CapabilityError::Unavailable(
        "too many redirects".to_owned(),
    ))
}

/// Resolves a redirect `Location` (absolute or root-relative) against the URL it
/// came from. Root-relative and same-scheme cases are enough for our fetches.
fn resolve_redirect(base: &str, location: &str) -> String {
    if location.starts_with("http://") || location.starts_with("https://") {
        return location.to_owned();
    }
    // Root-relative: keep scheme://host[:port], swap the path.
    if let Some(scheme_end) = base.find("://") {
        let after = &base[scheme_end + 3..];
        let authority_len = after.find('/').unwrap_or(after.len());
        let origin = &base[..scheme_end + 3 + authority_len];
        if location.starts_with('/') {
            return format!("{origin}{location}");
        }
        return format!("{origin}/{location}");
    }
    location.to_owned()
}

/// POSTs a JSON body and returns the response as text (size-capped).
fn http_post_json(url: &str, body: &Value, max_bytes: usize) -> Result<String, CapabilityError> {
    use std::io::Read;
    let mut resp = agent()
        .post(url)
        .send_json(body)
        .map_err(|e| CapabilityError::Unavailable(e.to_string()))?;
    let mut buf = Vec::new();
    resp.body_mut()
        .as_reader()
        .take(max_bytes as u64)
        .read_to_end(&mut buf)
        .map_err(|e| CapabilityError::Unavailable(e.to_string()))?;
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

/// Standard base64 encoding (no dependency) — for embedding image bytes in a JSON
/// request to a local vision model.
fn base64_encode(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(T[(n >> 18 & 63) as usize] as char);
        out.push(T[(n >> 12 & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            T[(n >> 6 & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            T[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

/// Resolves `{lat, lon}` or `{location}` from an input to a `(lat, lon, place)`,
/// geocoding a place name via Open-Meteo (no key). Shared by the location skills.
fn resolve_point(input: &Value) -> Result<(f64, f64, String), CapabilityError> {
    if let (Some(lat), Some(lon)) = (
        input.get("lat").and_then(Value::as_f64),
        input.get("lon").and_then(Value::as_f64),
    ) {
        let place = input
            .get("location")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        return Ok((lat, lon, place));
    }
    let q = str_field(input, "location")?;
    // A bare US ZIP ("10001") — common when the person types their postcode — is
    // not resolvable by the place-name geocoder, so use a keyless ZIP lookup.
    if let Some(point) = resolve_us_zip(q)? {
        return Ok(point);
    }
    // The Open-Meteo geocoder wants a bare city name — it returns nothing for
    // "New York NY" or "New York, NY". So try the full query, then simpler forms
    // (before a comma, and without a trailing US state abbreviation).
    let first = geocode_candidates(q)
        .into_iter()
        .find_map(|cand| {
            let geo = http_get_text(
                &format!(
                    "https://geocoding-api.open-meteo.com/v1/search?name={}&count=1&language=en&format=json",
                    urlencode(&cand)
                ),
                64 * 1024,
            )
            .ok()?;
            let geo: Value = serde_json::from_str(&geo).ok()?;
            geo["results"].get(0).cloned()
        })
        .ok_or_else(|| CapabilityError::BadInput(format!("couldn't find a place called '{q}'")))?;
    let lat = first["latitude"].as_f64().unwrap_or_default();
    let lon = first["longitude"].as_f64().unwrap_or_default();
    let name = first["name"].as_str().unwrap_or(q);
    let country = first["country"].as_str().unwrap_or("");
    Ok((
        lat,
        lon,
        format!("{name}, {country}")
            .trim_end_matches(", ")
            .to_owned(),
    ))
}

/// Progressively simpler place-name queries for the geocoder: the full string, the
/// part before a comma, and the same without a trailing 2-letter US state (so
/// "New York NY" / "New York, NY" both fall back to "New York").
fn geocode_candidates(q: &str) -> Vec<String> {
    let mut out = vec![q.trim().to_owned()];
    let before_comma = q.split(',').next().unwrap_or(q).trim();
    let toks: Vec<&str> = before_comma.split_whitespace().collect();
    let simplified = if toks.len() >= 2
        && toks
            .last()
            .is_some_and(|t| t.len() == 2 && t.chars().all(|c| c.is_ascii_uppercase()))
    {
        toks[..toks.len() - 1].join(" ")
    } else {
        before_comma.to_owned()
    };
    if !simplified.is_empty() && !out.contains(&simplified) {
        out.push(simplified);
    }
    out
}

/// If `q` is a 5-digit US ZIP, resolve it to `(lat, lon, "City, ST")` via the
/// keyless zippopotam.us service. Returns `Ok(None)` when `q` isn't a ZIP so the
/// caller falls through to the place-name geocoder.
fn resolve_us_zip(q: &str) -> Result<Option<(f64, f64, String)>, CapabilityError> {
    let zip = q.trim();
    if zip.len() != 5 || !zip.bytes().all(|b| b.is_ascii_digit()) {
        return Ok(None);
    }
    let body = http_get_text(&format!("https://api.zippopotam.us/us/{zip}"), 16 * 1024)?;
    let data: Value =
        serde_json::from_str(&body).map_err(|e| CapabilityError::Unavailable(e.to_string()))?;
    let place = data["places"]
        .get(0)
        .ok_or_else(|| CapabilityError::BadInput(format!("couldn't find the ZIP code '{zip}'")))?;
    // zippopotam returns latitude/longitude as strings.
    let lat = place["latitude"].as_str().and_then(|s| s.parse().ok());
    let lon = place["longitude"].as_str().and_then(|s| s.parse().ok());
    let (Some(lat), Some(lon)) = (lat, lon) else {
        return Ok(None);
    };
    let city = place["place name"].as_str().unwrap_or("");
    let state = place["state abbreviation"].as_str().unwrap_or("");
    Ok(Some((
        lat,
        lon,
        format!("{city}, {state}")
            .trim_matches([',', ' '])
            .to_owned(),
    )))
}

// ---- Weather (real; Open-Meteo, no API key) --------------------------------

struct WeatherCapability;

impl Capability for WeatherCapability {
    fn info(&self) -> CapabilityInfo {
        CapabilityInfo {
            id: "weather",
            name: "Weather",
            description: "Current conditions and today's forecast for a place, with a heads-up on severe weather.",
            category: "information",
            reaches_external: true,
            // Reads state; changes nothing. Policy-identical to Reversible
            // (both are Act), but it lets the turn tell an observation from a
            // receipt — see ADR 0053.
            reversibility: Reversibility::Observe,
            configured: true,
            needs: "",
            settings: &[],
            // Needs a place — a town, or a postcode, and said it needed nothing.
            input_schema: Some(
                r#"{"type":"object","properties":{"location":{"type":"string","description":"a town or postcode"},"lat":{"type":"number"},"lon":{"type":"number"}},"required":["location"]}"#,
            ),
        }
    }

    fn invoke(
        &self,
        input: &Value,
        _settings: &CapabilitySettings,
    ) -> Result<Value, CapabilityError> {
        // Accept {location:"..."} (a place name or US ZIP) or {lat, lon}.
        let (lat, lon, place) = resolve_point(input)?;

        let body = http_get_text(
            &format!(
                "https://api.open-meteo.com/v1/forecast?latitude={lat}&longitude={lon}\
                 &current=temperature_2m,apparent_temperature,weather_code,wind_speed_10m\
                 &daily=temperature_2m_max,temperature_2m_min,weather_code&timezone=auto&forecast_days=1"
            ),
            64 * 1024,
        )?;
        let w: Value =
            serde_json::from_str(&body).map_err(|e| CapabilityError::Unavailable(e.to_string()))?;
        let code = w["current"]["weather_code"].as_i64().unwrap_or(-1);
        let condition = weather_condition(code);
        let severe = matches!(code, 65 | 75 | 82 | 86 | 95 | 96 | 99);
        Ok(json!({
            "place": place,
            "temperature_c": w["current"]["temperature_2m"],
            "feels_like_c": w["current"]["apparent_temperature"],
            "wind_kmh": w["current"]["wind_speed_10m"],
            "condition": condition,
            "high_c": w["daily"]["temperature_2m_max"][0],
            "low_c": w["daily"]["temperature_2m_min"][0],
            // The local time this reading is FROM — a brief is posted once and never
            // updates, so "current" can go stale by the time it's read. Surfacing the
            // observation time makes that obvious ("as of 7 AM") instead of looking
            // like a wrong right-now temperature.
            "observed_at": w["current"]["time"],
            "warning": if severe { format!("Heads-up: {condition} expected — take care.") } else { String::new() },
        }))
    }

    fn summarize(&self, o: &Value) -> String {
        let place = o["place"]
            .as_str()
            .filter(|s| !s.is_empty())
            .unwrap_or("there");
        let cond = o["condition"].as_str().unwrap_or("");
        // Emit BOTH units so the model relays a grounded number instead of doing
        // its own (error-prone) C↔F conversion — whichever unit the person prefers,
        // the real value is right here.
        let cf = |c: f64| format!("{c:.0}°C / {:.0}°F", c * 9.0 / 5.0 + 32.0);
        let mut s = format!("Weather for {place}: {cond}");
        if let Some(t) = o["temperature_c"].as_f64() {
            s.push_str(&format!(", {}", cf(t)));
        }
        if let Some(f) = o["feels_like_c"].as_f64() {
            s.push_str(&format!(" (feels like {})", cf(f)));
        }
        // Time-stamp the reading so a stale brief reads as "as of this morning",
        // not a wrong current temperature.
        if let Some(at) = o["observed_at"].as_str().and_then(observed_time) {
            s.push_str(&format!(" as of {at}"));
        }
        if let (Some(hi), Some(lo)) = (o["high_c"].as_f64(), o["low_c"].as_f64()) {
            s.push_str(&format!("; today's high {}, low {}", cf(hi), cf(lo)));
        }
        if let Some(w) = o["warning"].as_str().filter(|w| !w.is_empty()) {
            s.push_str(&format!(". {w}"));
        }
        s
    }
}

/// Formats an Open-Meteo local ISO timestamp (`2026-07-23T07:00`) as a friendly
/// 12-hour time (`7:00 AM`), for the "as of …" tag on a weather reading. Returns
/// `None` if the shape isn't recognized.
fn observed_time(iso: &str) -> Option<String> {
    let hm = iso.split('T').nth(1)?;
    let (h, m) = hm.split_once(':')?;
    let h: u32 = h.parse().ok()?;
    if h > 23 || m.len() != 2 || !m.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let ampm = if h < 12 { "AM" } else { "PM" };
    let h12 = match h % 12 {
        0 => 12,
        x => x,
    };
    Some(format!("{h12}:{m} {ampm}"))
}

fn weather_condition(code: i64) -> &'static str {
    match code {
        0 => "clear sky",
        1..=3 => "partly cloudy",
        45 | 48 => "fog",
        51 | 53 | 55 | 56 | 57 => "drizzle",
        61 | 63 | 66 | 67 | 80 | 81 => "rain",
        65 | 82 => "heavy rain",
        71 | 73 | 77 | 85 => "snow",
        75 | 86 => "heavy snow",
        95 => "thunderstorm",
        96 | 99 => "thunderstorm with hail",
        _ => "unknown",
    }
}

fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            b' ' => "%20".to_owned(),
            other => format!("%{other:02X}"),
        })
        .collect()
}

// ---- Web fetch / browse (real) ---------------------------------------------

struct WebFetchCapability;

impl Capability for WebFetchCapability {
    fn info(&self) -> CapabilityInfo {
        CapabilityInfo {
            id: "web_fetch",
            name: "Web browsing",
            description: "Read ONE web page whose address you already have. It cannot search \
                          — if you do not have a real address, search first and read a result.",
            category: "information",
            reaches_external: true,
            // Reads state; changes nothing. Policy-identical to Reversible
            // (both are Act), but it lets the turn tell an observation from a
            // receipt — see ADR 0053.
            reversibility: Reversibility::Observe,
            configured: true,
            needs: "",
            settings: &[],
            // It requires a URL and said it took nothing, so the model supplied the one
            // address every documentation page uses — fetched `example.com`, got back "this
            // domain is for use in documentation examples", and answered from it. A skill
            // that fails is recoverable; one that succeeds against a placeholder is not.
            input_schema: Some(
                r#"{"type":"object","properties":{
                     "url":{"type":"string","description":"the full https:// address of a real page you already have"}},
                   "required":["url"]}"#,
            ),
        }
    }

    fn invoke(
        &self,
        input: &Value,
        _settings: &CapabilitySettings,
    ) -> Result<Value, CapabilityError> {
        let url = str_field(input, "url")?;
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return Err(CapabilityError::BadInput("url must be http(s)".to_owned()));
        }
        // Guard against SSRF: a model-provided URL must not reach internal hosts.
        let html = guarded_get_text(url, 512 * 1024)?;
        let title = between(&html, "<title>", "</title>").unwrap_or_default();
        let text = strip_html(&html);
        let excerpt: String = text.chars().take(2_000).collect();
        Ok(json!({ "url": url, "title": title.trim(), "text": excerpt }))
    }
}

fn between(s: &str, a: &str, b: &str) -> Option<String> {
    let start = s.find(a)? + a.len();
    let end = s[start..].find(b)? + start;
    Some(s[start..end].to_owned())
}

/// Very small HTML-to-text: drops script/style, strips tags, collapses space.
fn strip_html(html: &str) -> String {
    let mut out = String::with_capacity(html.len() / 2);
    let mut in_tag = false;
    let mut skip_to: Option<&str> = None;
    let lower = html.to_lowercase();
    let bytes = html.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if let Some(close) = skip_to {
            if lower[i..].starts_with(close) {
                skip_to = None;
                i += close.len();
            } else {
                i += 1;
            }
            continue;
        }
        if lower[i..].starts_with("<script") {
            skip_to = Some("</script>");
            continue;
        }
        if lower[i..].starts_with("<style") {
            skip_to = Some("</style>");
            continue;
        }
        let c = bytes[i] as char;
        match c {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                out.push(' ');
            }
            _ if !in_tag => out.push(c),
            _ => {}
        }
        i += 1;
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

// ---- Local news (real; Google News RSS, no API key) ------------------------

struct LocalNewsCapability;

impl Capability for LocalNewsCapability {
    fn info(&self) -> CapabilityInfo {
        CapabilityInfo {
            id: "news",
            name: "Local news",
            description: "Recent news headlines for a place or topic — so answers about the news are real, not guessed.",
            category: "information",
            reaches_external: true,
            // Reads state; changes nothing. Policy-identical to Reversible
            // (both are Act), but it lets the turn tell an observation from a
            // receipt — see ADR 0053.
            reversibility: Reversibility::Observe,
            configured: true,
            needs: "",
            settings: &[],
            // Needs where or what, and said it needed nothing.
            input_schema: Some(
                r#"{"type":"object","properties":{"query":{"type":"string","description":"a topic to search for"},"location":{"type":"string","description":"a town, when asking what is happening there"}},"required":[]}"#,
            ),
        }
    }

    fn invoke(
        &self,
        input: &Value,
        _settings: &CapabilitySettings,
    ) -> Result<Value, CapabilityError> {
        // Prefer an explicit {query}; else build one from {location}. A bare ZIP or
        // raw coordinates make a poor news search, so resolve the location to a
        // place name first ("10001" → "New York, NY news"). One of query/location
        // is required — without it we say so rather than invent headlines.
        let query = match input.get("query").and_then(Value::as_str) {
            Some(q) if !q.trim().is_empty() => q.trim().to_owned(),
            _ => {
                let raw = str_field(input, "location").map_err(|_| {
                    CapabilityError::BadInput("missing 'location' or 'query'".to_owned())
                })?;
                let place = match resolve_point(input) {
                    Ok((_, _, p)) if !p.is_empty() => p,
                    _ => raw.to_owned(),
                };
                format!("{place} news")
            }
        };
        let url = format!(
            "https://news.google.com/rss/search?q={}&hl=en-US&gl=US&ceid=US:en",
            urlencode(&query)
        );
        let xml = http_get_text(&url, 256 * 1024)?;
        let items = extract_rss_items(&xml, 6);
        let headlines: Vec<Value> = items
            .iter()
            .map(|(title, link, publisher)| {
                json!({ "title": title, "url": link, "publisher": publisher })
            })
            .collect();
        Ok(json!({
            "query": query,
            "count": headlines.len(),
            "headlines": headlines,
            "note": if headlines.is_empty() {
                "No recent headlines found for that search."
            } else {
                ""
            },
        }))
    }

    fn summarize(&self, o: &Value) -> String {
        let query = o["query"].as_str().unwrap_or("that");
        // A headline is either a plain string (legacy) or `{title, url}`. Include
        // the source URL when present so the butler can cite it, not guess it.
        let render = |i: usize, h: &Value| -> Option<String> {
            if let Some(t) = h.as_str() {
                return Some(format!("{}. {t}", i + 1));
            }
            let title = h["title"].as_str().filter(|s| !s.is_empty())?;
            let publisher = h["publisher"].as_str().filter(|s| !s.is_empty());
            let url = h["url"].as_str().filter(|s| !s.is_empty());
            // Cite the outlet + the link when we have them, so the butler relays a
            // real source rather than guessing one.
            let src = match (publisher, url) {
                (Some(p), Some(u)) => format!(" — {p} ({u})"),
                (Some(p), None) => format!(" — {p}"),
                (None, Some(u)) => format!(" — {u}"),
                (None, None) => String::new(),
            };
            Some(format!("{}. {title}{src}", i + 1))
        };
        let list: Vec<String> = o["headlines"]
            .as_array()
            .map(|a| {
                a.iter()
                    .enumerate()
                    .filter_map(|(i, h)| render(i, h))
                    .collect()
            })
            .unwrap_or_default();
        if list.is_empty() {
            return format!("No recent news headlines were found for {query}.");
        }
        format!("Recent news headlines for {query}:\n{}", list.join("\n"))
    }
}

/// Extracts up to `max` `<item>` headlines from an RSS feed as `(title, link,
/// publisher)`, decoding the common XML entities. The link is the article's
/// **source** and `publisher` the outlet name (Google News RSS carries a
/// `<source url="…">Publisher</source>`) — both kept so the butler can *cite* a
/// real source, not guess one. Small and tolerant.
fn extract_rss_items(xml: &str, max: usize) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(start) = rest.find("<item>") {
        rest = &rest[start + "<item>".len()..];
        let item = match rest.find("</item>") {
            Some(end) => &rest[..end],
            None => rest,
        };
        let title = between(item, "<title>", "</title>")
            .map(|t| decode_xml_entities(strip_cdata(&t).trim()))
            .unwrap_or_default();
        let link = between(item, "<link>", "</link>")
            .map(|l| decode_xml_entities(strip_cdata(&l).trim()))
            .unwrap_or_default();
        // <source url="…">Publisher</source> — take the text after the opening
        // tag's ">" (skips the url= attribute) up to the closing tag.
        let publisher = between(item, "<source", "</source>")
            .and_then(|s| s.split_once('>').map(|(_, name)| name.to_owned()))
            .map(|n| decode_xml_entities(strip_cdata(n.trim()).trim()))
            .unwrap_or_default();
        if !title.is_empty() {
            out.push((title, link, publisher));
        }
        if out.len() >= max {
            break;
        }
    }
    out
}

/// Unwraps a `<![CDATA[…]]>` wrapper if present, returning the inner text.
fn strip_cdata(s: &str) -> String {
    let t = s.trim();
    t.strip_prefix("<![CDATA[")
        .and_then(|r| r.strip_suffix("]]>"))
        .unwrap_or(t)
        .to_owned()
}

/// Decodes the handful of XML entities that show up in RSS titles.
fn decode_xml_entities(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
}

// ---- Knowledge lookup (real; Wikipedia, no API key) ------------------------

struct KnowledgeCapability;

impl Capability for KnowledgeCapability {
    fn info(&self) -> CapabilityInfo {
        CapabilityInfo {
            id: "knowledge",
            name: "Knowledge lookup",
            description: "Look up factual, encyclopedic knowledge about a topic, person, or place (Wikipedia).",
            category: "information",
            reaches_external: true,
            // Reads state; changes nothing. Policy-identical to Reversible
            // (both are Act), but it lets the turn tell an observation from a
            // receipt — see ADR 0053.
            reversibility: Reversibility::Observe,
            configured: true,
            needs: "",
            settings: &[],
            // Needs what to look up, and said it needed nothing.
            input_schema: Some(
                r#"{"type":"object","properties":{"query":{"type":"string","description":"the topic, person or place to look up"}},"required":["query"]}"#,
            ),
        }
    }

    fn invoke(
        &self,
        input: &Value,
        _settings: &CapabilitySettings,
    ) -> Result<Value, CapabilityError> {
        let q = str_field(input, "query").or_else(|_| str_field(input, "topic"))?;
        // Find the best-matching article title, then fetch its summary.
        let search = http_get_text_ua(
            &format!(
                "https://en.wikipedia.org/w/api.php?action=query&list=search&srsearch={}&srlimit=1&format=json",
                urlencode(q)
            ),
            WIKI_UA,
            128 * 1024,
        )?;
        let search: Value = serde_json::from_str(&search)
            .map_err(|e| CapabilityError::Unavailable(e.to_string()))?;
        let Some(title) = search["query"]["search"]
            .get(0)
            .and_then(|s| s["title"].as_str())
        else {
            return Ok(json!({ "query": q, "found": false, "extract": "" }));
        };
        let summary = http_get_text_ua(
            &format!(
                "https://en.wikipedia.org/api/rest_v1/page/summary/{}",
                urlencode(title)
            ),
            WIKI_UA,
            128 * 1024,
        )?;
        let summary: Value = serde_json::from_str(&summary)
            .map_err(|e| CapabilityError::Unavailable(e.to_string()))?;
        Ok(json!({
            "query": q,
            "found": true,
            "title": summary["title"].as_str().unwrap_or(title),
            "extract": summary["extract"].as_str().unwrap_or(""),
            "url": summary["content_urls"]["desktop"]["page"].as_str().unwrap_or(""),
        }))
    }

    fn summarize(&self, o: &Value) -> String {
        let extract = o["extract"].as_str().unwrap_or("");
        if extract.is_empty() {
            return format!(
                "I couldn't find an encyclopedia entry for '{}'.",
                o["query"].as_str().unwrap_or("that")
            );
        }
        let title = o["title"].as_str().unwrap_or("");
        format!("{title}: {extract}")
    }
}

const WIKI_UA: &str = "Endora personal butler (github.com/RusticPotatos/Endora)";
// ---- Image review (local vision model via Ollama; env-gated) ---------------

struct ImageReviewCapability {
    /// Base URL of the local Ollama (native API), e.g. `http://host:11434`.
    ollama_base: String,
}

impl ImageReviewCapability {
    fn from_env() -> Self {
        // Reuse the butler's model endpoint; the native /api lives at the base, so
        // strip the OpenAI-compat `/v1` suffix.
        let base = std::env::var("ENDORA_MODEL_URL")
            .unwrap_or_else(|_| "http://localhost:11434/v1".to_owned());
        let base = base
            .trim_end_matches('/')
            .trim_end_matches("/v1")
            .trim_end_matches('/')
            .to_owned();
        Self { ollama_base: base }
    }
}

const IMAGE_MODEL_SETTING: &[SettingSpec] = &[SettingSpec {
    key: "model",
    label: "Vision model (a pulled Ollama model, e.g. moondream or llava)",
    secret: false,
    optional: false,
}];

impl Capability for ImageReviewCapability {
    fn info(&self) -> CapabilityInfo {
        CapabilityInfo {
            id: "image_review",
            name: "Image review",
            description: "Describe or answer questions about an image, using a local vision model.",
            category: "media",
            reaches_external: false,
            // Reads state; changes nothing. Policy-identical to Reversible
            // (both are Act), but it lets the turn tell an observation from a
            // receipt — see ADR 0053.
            reversibility: Reversibility::Observe,
            // Code is ready; it becomes usable once the `model` setting is filled.
            configured: true,
            needs: "set the vision model (e.g. moondream) in this skill's settings",
            settings: IMAGE_MODEL_SETTING,
            input_schema: None,
        }
    }

    fn invoke(
        &self,
        input: &Value,
        settings: &CapabilitySettings,
    ) -> Result<Value, CapabilityError> {
        let model = settings
            .get("model")
            .map(String::as_str)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                CapabilityError::Unavailable(
                    "no vision model set — configure 'model' (e.g. moondream) in this skill's settings".to_owned(),
                )
            })?;
        let question = input
            .get("question")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .unwrap_or("Describe this image in detail.");
        // Accept a URL to fetch, or already-encoded base64.
        let image_b64 = if let Some(url) = input.get("image_url").and_then(Value::as_str) {
            // Guard against SSRF on a model/person-provided image URL.
            base64_encode(&guarded_get_bytes(url, 8 * 1024 * 1024)?)
        } else if let Some(b64) = input.get("image_base64").and_then(Value::as_str) {
            b64.to_owned()
        } else {
            return Err(CapabilityError::BadInput(
                "provide 'image_url' or 'image_base64'".to_owned(),
            ));
        };
        let body = http_post_json(
            &format!("{}/api/generate", self.ollama_base),
            &json!({ "model": model, "prompt": question, "images": [image_b64], "stream": false }),
            256 * 1024,
        )?;
        let v: Value =
            serde_json::from_str(&body).map_err(|e| CapabilityError::Unavailable(e.to_string()))?;
        Ok(json!({ "description": v["response"].as_str().unwrap_or("").trim() }))
    }

    fn summarize(&self, o: &Value) -> String {
        let d = o["description"].as_str().unwrap_or("").trim();
        if d.is_empty() {
            "I couldn't make out anything from that image.".to_owned()
        } else {
            d.to_owned()
        }
    }
}

// ---- Declared-but-unconfigured modules (scaffolds) -------------------------

/// A skill that is declared with its full metadata but awaits a data source or
/// key. It appears in the registry as "needs setup" rather than being missing.
macro_rules! scaffold {
    ($ty:ident, $id:literal, $name:literal, $desc:literal, $cat:literal, $external:literal, $reversibility:expr, $needs:literal) => {
        struct $ty;
        impl Capability for $ty {
            fn info(&self) -> CapabilityInfo {
                CapabilityInfo {
                    id: $id,
                    name: $name,
                    description: $desc,
                    category: $cat,
                    reaches_external: $external,
                    reversibility: $reversibility,
                    configured: false,
                    needs: $needs,
                    settings: &[],
                    input_schema: None,
                }
            }
            fn invoke(
                &self,
                _input: &Value,
                _settings: &CapabilitySettings,
            ) -> Result<Value, CapabilityError> {
                Err(CapabilityError::Unavailable(format!(
                    "{} needs setup: {}",
                    $name, $needs
                )))
            }
        }
    };
}

/// Today, as `YYYY-MM-DD`.
///
/// The clock does not reach this layer as a port the way it does in the domain — a capability
/// is an adapter to somebody else's service and is allowed to know what day it is. Kept in one
/// place so the format is not written twice.
fn today_utc() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default();
    // Civil-from-days: the standard algorithm, so a date is arithmetic rather than a
    // dependency for one line of formatting.
    let days = (secs / 86_400) as i64;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

/// A failure with the credential taken back out of it.
///
/// This API takes its key in the **query string** — its choice, not ours — so the key is
/// part of the URL of every request. Whether a failure repeats that URL is then decided by
/// the `Display` of an HTTP crate, and a message like *"401 for
/// https://…?apikey=REAL_KEY&…"* would land in an error the person reads and in the record
/// of what was tried.
///
/// So it is removed rather than hoped about. The dependency may print whatever it likes;
/// the key cannot survive this function, and what it says about *why* it failed — a 401 is
/// the most useful thing here — survives intact.
#[must_use]
pub fn without_the_key(said: &str, key: &str) -> String {
    if key.trim().is_empty() {
        return said.to_owned();
    }
    said.replace(key.trim(), "…")
}

/// One ticketed event, as a person would hear it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TicketedEvent {
    /// What it is called.
    pub what: String,
    /// `YYYY-MM-DD`.
    pub on: String,
    /// Local start time, `HH:MM`, empty when the listing has not said.
    pub at: String,
    /// Where, plus its town when the listing gives one.
    pub place: String,
    /// The cheapest advertised ticket, already rounded, empty when none is published.
    pub from: String,
}

/// The events in a Discovery API answer.
///
/// Parsed, never trusted. Field names are the whole of the contract with somebody else's
/// service, and a renamed one has to yield **no events** rather than an error or a
/// half-built row — the same posture as reading a city's agenda, and for the same reason: a
/// screen quietly saying "nothing on" is the worst failure available to a thing whose only
/// job is to say what is on.
#[must_use]
pub fn ticketed_events_in(body: &str) -> Vec<TicketedEvent> {
    let Ok(parsed) = serde_json::from_str::<Value>(body) else {
        return Vec::new();
    };
    let Some(Value::Array(rows)) = parsed.get("_embedded").and_then(|e| e.get("events")) else {
        // No `_embedded` at all is what an empty search returns, and it is not an error.
        return Vec::new();
    };
    rows.iter()
        .filter_map(|row| {
            // Something nobody can name is not worth reporting; the rest is optional.
            let what = row.get("name")?.as_str()?.trim().to_owned();
            if what.is_empty() {
                return None;
            }
            let start = row.get("dates").and_then(|d| d.get("start"));
            let venue = row
                .get("_embedded")
                .and_then(|e| e.get("venues"))
                .and_then(Value::as_array)
                .and_then(|v| v.first());
            let place = match (
                venue.and_then(|v| v.get("name")).and_then(Value::as_str),
                venue
                    .and_then(|v| v.get("city"))
                    .and_then(|c| c.get("name"))
                    .and_then(Value::as_str),
            ) {
                (Some(name), Some(town)) if !name.is_empty() && !town.is_empty() => {
                    format!("{name}, {town}")
                }
                (Some(name), _) => name.to_owned(),
                (None, Some(town)) => town.to_owned(),
                _ => String::new(),
            };
            Some(TicketedEvent {
                what,
                place,
                on: start
                    .and_then(|s| s.get("localDate"))
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                // `20:00:00` is a machine's way of saying eight o'clock; the seconds are noise.
                at: start
                    .and_then(|s| s.get("localTime"))
                    .and_then(Value::as_str)
                    .map(|t| t.split(':').take(2).collect::<Vec<_>>().join(":"))
                    .unwrap_or_default(),
                from: row
                    .get("priceRanges")
                    .and_then(Value::as_array)
                    .and_then(|p| p.first())
                    .and_then(|p| p.get("min"))
                    .and_then(Value::as_f64)
                    .map(|m| format!("{}", m.round() as i64))
                    .unwrap_or_default(),
            })
        })
        .collect()
}

/// Says a list the way a person would, not the way a printer would.
///
/// Asked for significant events, the butler recited six council committees verbatim — a
/// summariser that joins every row with "; " hands a small model a list, and a small model
/// handed a list reads the list. Proportion is the fix and it is universal: few enough to
/// say, say them; more than that, say the shape and the first few, and count the rest.
#[must_use]
pub fn said_proportionately(said: Vec<String>, what: &str) -> String {
    /// Up to this many, naming them all reads fine.
    const FEW_ENOUGH_TO_SAY: usize = 4;
    if said.is_empty() {
        return format!("Nothing {what} just now.");
    }
    if said.len() <= FEW_ENOUGH_TO_SAY {
        return said.join("; ");
    }
    let rest = said.len() - FEW_ENOUGH_TO_SAY;
    format!(
        "{} of them — {}; and {rest} more",
        said.len(),
        said[..FEW_ENOUGH_TO_SAY].join("; ")
    )
}

/// Says an event the way somebody would mention it.
#[must_use]
pub fn describe_ticketed_event(e: &TicketedEvent) -> String {
    let mut said = e.what.clone();
    if !e.on.is_empty() {
        said.push_str(&format!(" on {}", e.on));
    }
    if !e.at.is_empty() {
        said.push_str(&format!(" at {}", e.at));
    }
    if !e.place.is_empty() {
        said.push_str(&format!(", {}", e.place));
    }
    if !e.from.is_empty() {
        said.push_str(&format!(", from ${}", e.from));
    }
    said
}

/// One public meeting, as a person would hear it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicMeeting {
    /// `2026-08-03`.
    pub on: String,
    /// `9:00 AM`, in the body's own words. Empty when it does not say.
    pub at: String,
    /// Which committee or council.
    pub who: String,
    /// Where it sits. Empty when it does not say.
    pub place: String,
}

/// Reads Legistar's answer into meetings.
///
/// Legistar runs the legislative calendar for a great many US municipalities and its Web API
/// is **open and keyless** — which is why the civic half of "what's on this week" needs no
/// account, unlike everything else in this area.
///
/// It is also why this is parsed rather than trusted: field names are the only contract, a
/// renamed one would yield empty meetings rather than an error, and a screen quietly saying
/// "nothing on" is the worst possible failure for a thing whose whole job is to say what is on.
#[must_use]
pub fn meetings_in(body: &str) -> Vec<PublicMeeting> {
    let Ok(Value::Array(rows)) = serde_json::from_str::<Value>(body) else {
        return Vec::new();
    };
    rows.iter()
        .filter_map(|row| {
            // A meeting nobody can name is not worth reporting; everything else is optional.
            let who = row.get("EventBodyName")?.as_str()?.trim().to_owned();
            if who.is_empty() {
                return None;
            }
            let on = row
                .get("EventDate")
                .and_then(Value::as_str)
                // Legistar dates arrive as `2026-08-03T00:00:00`; the day is the useful part
                // and the zeroed time is noise that reads as a real
                .map(|d| d.split('T').next().unwrap_or(d).to_owned())
                .unwrap_or_default();
            Some(PublicMeeting {
                on,
                at: row
                    .get("EventTime")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .trim()
                    .to_owned(),
                who,
                place: row
                    .get("EventLocation")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .trim()
                    .to_owned(),
            })
        })
        .collect()
}

/// Says a meeting the way somebody would mention it.
#[must_use]
pub fn describe_meeting(m: &PublicMeeting) -> String {
    let when = match (m.on.is_empty(), m.at.is_empty()) {
        (true, _) => String::new(),
        (false, true) => format!(" on {}", m.on),
        (false, false) => format!(" on {} at {}", m.on, m.at),
    };
    let where_ = if m.place.is_empty() {
        String::new()
    } else {
        format!(", {}", m.place)
    };
    format!("{}{when}{where_}", m.who)
}

/// What the city is doing this week — public meetings from Legistar.
///
/// Named for what it can actually answer rather than for the whole of "local events". The
/// stub this replaces promised "concerts, markets, community happenings" and delivered
/// nothing; ticketed events are a separate source with a separate key, and pretending one
/// skill covers both is how a person ends up asking a question it was never going to answer.
/// What is on at a venue — concerts, sport, shows — from Ticketmaster's Discovery API.
///
/// **ADR 0058 says MCP by default, and this is native anyway.** The rule is *"answers, or a
/// relationship?"*, and this is plainly answers — so a table row should have beaten a
/// thousand lines of Rust. What decides it is the state of the shelf: the only Ticketmaster
/// server published is a v0.1.0 **remote gateway** from a publisher nobody knows, with no
/// local package. Using it routes either the credential or every query — which city, which
/// venue, what somebody is interested in — through a stranger, to reach a public REST API
/// that answers a single authenticated GET.
///
/// That inverts 0058's cost calculus rather than escaping it: native is Rust owned forever,
/// and a row in a table is cheap *only when the row is trustworthy*.
///
/// **So this is deliberately revisitable.** If Ticketmaster publishes a first-party server,
/// or anyone ships one that runs locally over stdio, delete this and add a catalogue entry.
/// Native has to keep earning it.
///
/// Named for what it holds. The skill next door was called `local_events` while answering
/// only with civic agendas, and the butler reached for it for a stadium question twice
/// because on a small model the id is the headline.
struct TicketedEventsCapability;

impl Capability for TicketedEventsCapability {
    fn info(&self) -> CapabilityInfo {
        CapabilityInfo {
            id: "ticketed_events",
            name: "What's on at a venue",
            description: "Concerts, sport, shows and theatre — with dates, venue and the \
                          cheapest ticket. Ticketed events ONLY: it knows nothing about \
                          council meetings, community listings, or anything without a \
                          ticket. Give it a venue, a team, an act or just a town.",
            category: "information",
            reaches_external: true,
            reversibility: Reversibility::Observe,
            configured: true,
            needs: "",
            settings: &[SettingSpec {
                key: "ticketmaster_key",
                label: "your Ticketmaster consumer key",
                // It travels in a query string, because that is the only way this API takes
                // it. A URL reaches logs far more readily than a header does, so it is
                // stored as a secret and never echoed back — see `self_test`, which reports
                // what came back and never what was sent.
                secret: true,
                optional: false,
            }],
            // Named fields, because the model is otherwise guessing. It was offered this
            // skill with an empty parameter object, called it with `{}` twice, and was told
            // "say what to look for" twice — doing exactly what it had been told it could.
            input_schema: Some(
                r#"{"type":"object","properties":{
                     "what":{"type":"string","description":"a venue, a team, an act, or a show"},
                     "city":{"type":"string","description":"the town to look in"}},
                   "required":[]}"#,
            ),
        }
    }

    fn invoke(
        &self,
        input: &Value,
        settings: &CapabilitySettings,
    ) -> Result<Value, CapabilityError> {
        let key = settings
            .get("ticketmaster_key")
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                CapabilityError::Unavailable(
                    "I need a Ticketmaster consumer key before I can see what's on —                      developer.ticketmaster.com, My Apps."
                        .to_owned(),
                )
            })?;
        // What to look for: a venue, a team, an act. A town narrows it when given.
        let asked = input
            .get("what")
            .or_else(|| input.get("keyword"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim();
        let town = input
            .get("city")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim();
        if asked.is_empty() && town.is_empty() {
            return Err(CapabilityError::BadInput(
                "say what to look for — a venue, a team, an act, or a town".to_owned(),
            ));
        }
        // Only what has not happened yet. A listing service will happily answer "what is on"
        // with last spring, which is worse than answering nothing.
        let from = format!("{}T00:00:00Z", crate::infrastructure::today_utc());
        let mut url = format!(
            "https://app.ticketmaster.com/discovery/v2/events.json\
             ?apikey={key}&startDateTime={from}&size=20&sort=date,asc"
        );
        if !asked.is_empty() {
            url.push_str(&format!("&keyword={}", urlencode(asked)));
        }
        if !town.is_empty() {
            url.push_str(&format!("&city={}", urlencode(town)));
        }
        let body = http_get_text_ua(
            &url,
            "Endora personal butler (github.com/RusticPotatos/Endora)",
            512 * 1024,
        )
        .map_err(|e| match e {
            CapabilityError::Unavailable(m) => {
                CapabilityError::Unavailable(without_the_key(&m, key))
            }
            CapabilityError::BadInput(m) => CapabilityError::BadInput(without_the_key(&m, key)),
        })?;
        let on = ticketed_events_in(&body);
        Ok(serde_json::json!({
            "events": on
                .iter()
                .map(|e| serde_json::json!({
                    "what": e.what, "on": e.on, "at": e.at,
                    "place": e.place, "from": e.from,
                }))
                .collect::<Vec<_>>(),
        }))
    }

    fn summarize(&self, output: &Value) -> String {
        let on: Vec<TicketedEvent> = output
            .get("events")
            .and_then(Value::as_array)
            .map(|all| {
                all.iter()
                    .map(|e| TicketedEvent {
                        what: e["what"].as_str().unwrap_or_default().to_owned(),
                        on: e["on"].as_str().unwrap_or_default().to_owned(),
                        at: e["at"].as_str().unwrap_or_default().to_owned(),
                        place: e["place"].as_str().unwrap_or_default().to_owned(),
                        from: e["from"].as_str().unwrap_or_default().to_owned(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        if on.is_empty() {
            return "Nothing ticketed coming up for that.".to_owned();
        }
        said_proportionately(
            on.iter().map(describe_ticketed_event).collect(),
            "ticketed coming up",
        )
    }

    /// Proves the key works, without ever repeating it.
    ///
    /// A wrong key here is a 401 from Ticketmaster, which is the single most useful thing
    /// this can report — the same reason an MCP server got a Test button.
    fn self_test(&self, settings: &CapabilitySettings) -> Result<String, CapabilityError> {
        let found = self.invoke(&serde_json::json!({ "city": "New York" }), settings)?;
        let n = found
            .get("events")
            .and_then(Value::as_array)
            .map_or(0, Vec::len);
        Ok(format!("The key works — {n} listings came back."))
    }
}

struct CityMeetingsCapability;

impl Capability for CityMeetingsCapability {
    fn info(&self) -> CapabilityInfo {
        CapabilityInfo {
            id: "city_meetings",
            name: "What the city is doing",
            description: "Council, committee, planning and zoning meetings from the city's \
                          own agenda. Civic business ONLY — it does not know about concerts, \
                          sport, shows or anything at a venue.",
            category: "information",
            reaches_external: true,
            reversibility: Reversibility::Observe,
            configured: true,
            needs: "",
            settings: &[SettingSpec {
                key: "legistar_client",
                label: "your city's Legistar name (e.g. seattle)",
                secret: false,
                optional: false,
            }],
            input_schema: None,
        }
    }

    fn invoke(
        &self,
        _input: &Value,
        settings: &CapabilitySettings,
    ) -> Result<Value, CapabilityError> {
        let client = settings
            .get("legistar_client")
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                CapabilityError::Unavailable(
                    "tell me your city's Legistar name — the part before .legistar.com in its \
                     meetings page address"
                        .to_owned(),
                )
            })?;
        // Only what has not happened yet. Legistar holds a decade of history and answering
        // "what is on this week" with 2015 would be worse than answering nothing.
        let from = crate::infrastructure::today_utc();
        let url = format!(
            "https://webapi.legistar.com/v1/{client}/events?$filter=EventDate%20ge%20datetime'{from}'&$orderby=EventDate&$top=25"
        );
        let body = http_get_text_ua(
            &url,
            "Endora personal butler (github.com/RusticPotatos/Endora)",
            512 * 1024,
        )?;
        let meetings = meetings_in(&body);
        Ok(serde_json::json!({
            "meetings": meetings
                .iter()
                .map(|m| serde_json::json!({
                    "on": m.on, "at": m.at, "who": m.who, "place": m.place,
                }))
                .collect::<Vec<_>>(),
        }))
    }

    fn summarize(&self, output: &Value) -> String {
        let meetings: Vec<PublicMeeting> = output
            .get("meetings")
            .and_then(Value::as_array)
            .map(|all| {
                all.iter()
                    .map(|m| PublicMeeting {
                        on: m["on"].as_str().unwrap_or_default().to_owned(),
                        at: m["at"].as_str().unwrap_or_default().to_owned(),
                        who: m["who"].as_str().unwrap_or_default().to_owned(),
                        place: m["place"].as_str().unwrap_or_default().to_owned(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        if meetings.is_empty() {
            return "Nothing on the city's public calendar just now.".to_owned();
        }
        said_proportionately(
            meetings.iter().map(describe_meeting).collect(),
            "on the city's public calendar",
        )
    }
}
scaffold!(
    FlightSearchCapability,
    "flights",
    "Flight search",
    "Find and compare flights for a trip.",
    "travel",
    true,
    Reversibility::Irreversible, // booking spends money and can't be undone
    "a flights API key (booking stays a human decision)"
);
scaffold!(
    LocationLogCapability,
    "location",
    "Location tracking",
    "Keep a private log of where you are while travelling, so the butler has context.",
    "presence",
    false,
    Reversibility::OutwardReversible, // a private log you can delete
    "your opt-in and a location source (kept private to you)"
);
/// The "guard dog": active public-safety alerts near a place. Real for the US via
/// the National Weather Service (no key); elsewhere it reports no coverage.
struct SafetyAlertsCapability;

impl Capability for SafetyAlertsCapability {
    fn info(&self) -> CapabilityInfo {
        CapabilityInfo {
            id: "safety_alerts",
            name: "Guard dog",
            description: "Active safety alerts near you — severe weather and public warnings (US National Weather Service).",
            category: "safety",
            reaches_external: true,
            // Reads state; changes nothing. Policy-identical to Reversible
            // (both are Act), but it lets the turn tell an observation from a
            // receipt — see ADR 0053.
            reversibility: Reversibility::Observe,
            configured: true,
            needs: "",
            settings: &[],
            // Needs where, and said it needed nothing.
            input_schema: Some(
                r#"{"type":"object","properties":{"location":{"type":"string","description":"a town or postcode"},"lat":{"type":"number"},"lon":{"type":"number"}},"required":["location"]}"#,
            ),
        }
    }

    fn invoke(
        &self,
        input: &Value,
        _settings: &CapabilitySettings,
    ) -> Result<Value, CapabilityError> {
        let (lat, lon, place) = resolve_point(input)?;
        let body = http_get_text_ua(
            &format!("https://api.weather.gov/alerts/active?point={lat:.4},{lon:.4}"),
            "Endora personal butler (github.com/RusticPotatos/Endora)",
            256 * 1024,
        )?;
        let data: Value =
            serde_json::from_str(&body).map_err(|e| CapabilityError::Unavailable(e.to_string()))?;
        let alerts: Vec<Value> = data["features"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|f| {
                        let p = &f["properties"];
                        Some(json!({
                            "event": p["event"].as_str()?,
                            "severity": p["severity"].as_str().unwrap_or("Unknown"),
                            "headline": p["headline"].as_str().unwrap_or(""),
                            "area": p["areaDesc"].as_str().unwrap_or(""),
                        }))
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(json!({
            "place": place,
            "count": alerts.len(),
            "all_clear": alerts.is_empty(),
            "alerts": alerts,
            "note": if alerts.is_empty() { "No active alerts (or outside US coverage)." } else { "" },
        }))
    }

    fn summarize(&self, o: &Value) -> String {
        let place = o["place"]
            .as_str()
            .filter(|s| !s.is_empty())
            .unwrap_or("there");
        let alerts: Vec<String> = o["alerts"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|x| {
                        let event = x["event"].as_str()?;
                        let sev = x["severity"].as_str().unwrap_or("");
                        Some(if sev.is_empty() {
                            event.to_owned()
                        } else {
                            format!("{event} ({sev})")
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        if alerts.is_empty() {
            return format!("No active safety alerts for {place} right now.");
        }
        format!("Active safety alerts for {place}: {}.", alerts.join("; "))
    }
}

scaffold!(
    IncidentScannerCapability,
    "incident_scanner",
    "Incident scanner",
    "Surface public emergency/incident alerts nearby (fire, rescue, major incidents).",
    "safety",
    true,
    Reversibility::Reversible, // read-only lookup
    "a public incident/emergency feed for your area"
);

// ---- Home Assistant (read-only; learn the home's routines) -----------------

const HA_SETTINGS: &[SettingSpec] = &[
    SettingSpec {
        key: "url",
        label: "Home Assistant URL (e.g. http://homeassistant.local:8123)",
        secret: false,
        optional: false,
    },
    SettingSpec {
        key: "token",
        label: "Long-lived access token",
        secret: true,
        optional: false,
    },
    SettingSpec {
        key: "mcp_server",
        label: "Name of the matching MCP server (blank = home-assistant)",
        secret: false,
        optional: true,
    },
    SettingSpec {
        key: "write_names",
        label: "Let Endora write names back into Home Assistant (on/off)",
        secret: false,
        optional: false,
    },
    SettingSpec {
        key: "busy_entity",
        label: "How Endora can tell you don't want interrupting — an entity that's on when \
                you're busy, e.g. binary_sensor.yourphone_focus (blank = always reach you)",
        secret: false,
        optional: true,
    },
    SettingSpec {
        key: "open_on_tap",
        label: "Where a tapped notification takes you — the address you reach Endora at, \
                e.g. https://192.168.1.10:8787 (blank = it just opens Home Assistant)",
        secret: false,
        optional: true,
    },
    SettingSpec {
        key: "notify_service",
        label: "How Endora reaches you when you're away — a notify service, e.g.                 mobile_app_yourphone (blank = never)",
        secret: false,
        optional: true,
    },
];

/// Presentation and plumbing, which say nothing about the thing itself.
///
/// `friendly_name` is already the name; `supported_features` is a bitmask; `attribution` and
/// `description` are boilerplate a service attaches to every reading.
const NOT_ABOUT_THE_THING: &[&str] = &[
    "friendly_name",
    "supported_features",
    "attribution",
    "icon",
    "entity_picture",
    "editable",
    "device_class",
    "hidden_by",
    "assumed_state",
    "restored",
    "id",
    "description",
];

/// The attributes of a thing that carry its meaning, when its **state does not**.
///
/// Connecting a calendar achieved nothing until this existed. A calendar's state is `off`,
/// and its event — *"Jane Doe & John Doe at 18:30"* — lives entirely in its attributes,
/// so Endora read `Family: off` and had nothing to say about the person's evening. The same
/// holds for weather (`clear-night`, with the temperature in an attribute) and for a media
/// player (`playing`, with what is playing in one).
///
/// Deliberately **not a list of which attributes matter per kind of thing** — that would be
/// per-integration knowledge, and there are hundreds of kinds. Three generic rules do it:
///
/// - a **scalar**, so lists and nested objects are left out (a TV's `source_list` is forty
///   app names, and a forecast is an array of days);
/// - **not empty**, so a calendar with no location does not report an empty one;
/// - **short**, and dropped rather than truncated when it is not — a 500-character
///   `description` clipped to sixty is still sixty characters of nothing.
///
/// Capped per thing, because everything here is paid for on every turn that reads it — the
/// same budget that made a clock reading arriving with five kilobytes of house a bug
/// ([0053](../../docs/adr/0053-honesty-about-what-it-did.md)).
pub(crate) fn facts_worth_reading(attributes: &Value) -> serde_json::Map<String, Value> {
    /// Long enough for a time, a title or a temperature; short enough that nothing
    /// discursive gets in.
    const SHORT_ENOUGH: usize = 60;
    /// Enough to describe a thing, few enough that sixty things still fit in a reading.
    const ENOUGH_PER_THING: usize = 8;
    let Some(all) = attributes.as_object() else {
        return serde_json::Map::new();
    };
    all.iter()
        .filter(|(key, _)| !NOT_ABOUT_THE_THING.contains(&key.as_str()))
        .filter(|(_, value)| !value.is_array() && !value.is_object() && !value.is_null())
        .filter(|(_, value)| {
            let shown = value
                .as_str()
                .map_or_else(|| value.to_string(), ToOwned::to_owned);
            !shown.trim().is_empty() && shown.len() <= SHORT_ENOUGH
        })
        .take(ENOUGH_PER_THING)
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

/// Reads Home Assistant state so the butler can learn the home's routines (lights,
/// presence, sensors). Read-only and reversible — it observes, it does not actuate;
/// controlling devices/scripts is a separate, confirm-gated capability (ADR 0051).
struct HomeAssistantCapability;

impl Capability for HomeAssistantCapability {
    fn info(&self) -> CapabilityInfo {
        CapabilityInfo {
            id: "home_assistant",
            name: "Home Assistant",
            description: "Read your home's state — lights, presence, sensors — to learn your routines.",
            category: "presence",
            reaches_external: true,
            // Reads state; changes nothing. Policy-identical to Reversible
            // (both are Act), but it lets the turn tell an observation from a
            // receipt — see ADR 0053.
            reversibility: Reversibility::Observe,
            configured: true,
            needs: "your Home Assistant URL and a long-lived access token",
            settings: HA_SETTINGS,
            input_schema: None,
        }
    }

    /// Proves the whole chain in one press: it can reach Home Assistant, it can read, and
    /// — if a notify service has been nominated — it can reach the person.
    ///
    /// The notification is sent **because** the button was pressed, which is the only
    /// honest way to test that path: a nominated service that is misspelled fails silently
    /// forever otherwise, and the failure looks exactly like "nothing worth saying
    /// happened" (ADR 0056).
    fn self_test(&self, settings: &CapabilitySettings) -> Result<String, CapabilityError> {
        let reading = self.invoke(&Value::Object(serde_json::Map::new()), settings)?;
        let seen = reading
            .get("count")
            .and_then(Value::as_u64)
            .map_or_else(|| self.summarize(&reading), |n| format!("{n} things"));
        let Some(home) = crate::home_assistant::HomeAssistant::from_settings(settings) else {
            return Ok(format!("Connected. Endora can see {seen}."));
        };
        match home.notify("Endora", "Test from Endora — this is how I'll reach you.") {
            None => Ok(format!(
                "Connected. Endora can see {seen}. No notify service is set, so it has no \
                 way to reach you when you're away."
            )),
            Some(Ok(())) => Ok(format!(
                "Connected. Endora can see {seen}, and it just sent a test notification — \
                 check your phone."
            )),
            Some(Err(why)) => Err(CapabilityError::Unavailable(format!(
                "Endora can see {seen}, but the notify service did not work: {why}"
            ))),
        }
    }

    /// Home Assistant is the one service Endora needs a relationship with rather than answers
    /// from — it watches every entity for the world to change, takes presence from it into
    /// every turn, writes names back into it and can undo them.
    ///
    /// The alias filtering lives here rather than in the caller: which confirmed names belong
    /// to this server is Home-Assistant knowledge, and putting it in shared registration code
    /// is precisely the per-integration patch ADR 0054 was written about.
    fn channel(
        &self,
        settings: &CapabilitySettings,
        aliases: &[crate::domain::TargetAlias],
    ) -> Option<(String, std::sync::Arc<dyn NativeChannel>)> {
        let home = crate::home_assistant::HomeAssistant::from_settings(settings)?;
        let server = crate::home_assistant::paired_server(settings);
        // Every name a thing answers to, not just the service's own (ADR 0054) — the same
        // confirmed aliases the retry uses.
        let named: Vec<(String, String)> = aliases
            .iter()
            .filter(|a| a.server == server)
            .map(|a| (a.said.clone(), a.means.clone()))
            .collect();
        Some((server, std::sync::Arc::new(home.also_known_as(named))))
    }

    fn invoke(
        &self,
        input: &Value,
        settings: &CapabilitySettings,
    ) -> Result<Value, CapabilityError> {
        let base = settings
            .get("url")
            .map(|s| as_url(s))
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                CapabilityError::Unavailable(
                    "set the Home Assistant URL in this skill's settings".to_owned(),
                )
            })?;
        let token = settings
            .get("token")
            .map(String::as_str)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                CapabilityError::Unavailable(
                    "set a Home Assistant access token in this skill's settings".to_owned(),
                )
            })?;
        // Optional {domain} filter (e.g. "light", "person", "sensor", "media_player").
        let domain = input.get("domain").and_then(Value::as_str).unwrap_or("");
        // HA is the person's own local service — direct agent (not proxied/guarded).
        use std::io::Read;
        let mut resp = agent()
            .get(&format!("{base}/api/states"))
            .header("Authorization", &format!("Bearer {token}"))
            .call()
            .map_err(|e| CapabilityError::Unavailable(e.to_string()))?;
        let mut buf = Vec::new();
        resp.body_mut()
            .as_reader()
            .take(1024 * 1024)
            .read_to_end(&mut buf)
            .map_err(|e| CapabilityError::Unavailable(e.to_string()))?;
        let states: Value = serde_json::from_str(&String::from_utf8_lossy(&buf))
            .map_err(|e| CapabilityError::Unavailable(e.to_string()))?;
        let entities: Vec<Value> = states
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter(|e| {
                        domain.is_empty()
                            || e["entity_id"]
                                .as_str()
                                .is_some_and(|id| id.starts_with(&format!("{domain}.")))
                    })
                    .filter_map(|e| {
                        let id = e["entity_id"].as_str()?;
                        let name = e["attributes"]["friendly_name"].as_str().unwrap_or(id);
                        let mut about = json!({
                            "entity": id,
                            "name": name,
                            "state": e["state"].as_str().unwrap_or("?"),
                            "changed": e["last_changed"].as_str().unwrap_or(""),
                        });
                        let facts = facts_worth_reading(&e["attributes"]);
                        if !facts.is_empty() {
                            about["about"] = Value::Object(facts);
                        }
                        Some(about)
                    })
                    .take(60)
                    .collect()
            })
            .unwrap_or_default();
        Ok(json!({ "domain": domain, "count": entities.len(), "entities": entities }))
    }

    fn summarize(&self, o: &Value) -> String {
        let entities: Vec<&Value> = o["entities"]
            .as_array()
            .map(|a| a.iter().collect())
            .unwrap_or_default();
        if entities.is_empty() {
            return "No matching Home Assistant entities found.".to_owned();
        }
        let list = entities
            .iter()
            .take(30)
            .map(|e| {
                let said = format!(
                    "{}: {}",
                    e["name"].as_str().unwrap_or("?"),
                    e["state"].as_str().unwrap_or("?")
                );
                // The facts travel with it, because for many things the state alone says
                // nothing: `Family: off` is a calendar with an event in an hour.
                match e["about"].as_object().filter(|a| !a.is_empty()) {
                    None => said,
                    Some(about) => {
                        let detail = about
                            .iter()
                            .map(|(k, v)| {
                                format!(
                                    "{k}={}",
                                    v.as_str().map_or_else(|| v.to_string(), ToOwned::to_owned)
                                )
                            })
                            .collect::<Vec<_>>()
                            .join(", ");
                        format!("{said} ({detail})")
                    }
                }
            })
            .collect::<Vec<_>>()
            .join("; ");
        format!(
            "Home state ({} entities): {list}",
            o["count"].as_i64().unwrap_or(0)
        )
    }
}

// ---- Application-facing runner ---------------------------------------------

/// Adapts the concrete capability registry to the application's
/// [`CapabilityRunner`] port, so the butler use case can list and run skills
/// without depending on this crate. A capability is "autonomous" (may run on its
/// own this turn) exactly when the classifier's verdict for its [`Reversibility`]
/// band and the person's envelope is [`Act`](Decision::Act). Anything that must
/// confirm — or is blocked — stays gated.
pub struct RegistryRunner {
    capabilities: Arc<Vec<Arc<dyn Capability>>>,
    /// The person's per-tool stance (ADR 0062). Missing = the band's default.
    stances: std::collections::HashMap<String, Stance>,
    /// Tools the record has proven — enough read-back confirmed changes (ADR 0062).
    /// Derived from outcomes at composition, never stored.
    proven: std::collections::HashSet<String>,
    /// The person's autonomy envelope — the boundary the butler acts within.
    envelope: crate::application::AutonomyEnvelope,
    /// Per-capability settings (id → key/value), for skills that need config.
    settings: std::collections::HashMap<String, CapabilitySettings>,
}

impl RegistryRunner {
    /// Wraps a shared capability registry at its defaults: no stances stored, nothing
    /// proven, the default envelope, no settings.
    #[must_use]
    pub fn new(capabilities: Arc<Vec<Arc<dyn Capability>>>) -> Self {
        Self {
            capabilities,
            stances: std::collections::HashMap::new(),
            proven: std::collections::HashSet::new(),
            envelope: crate::application::AutonomyEnvelope::default(),
            settings: std::collections::HashMap::new(),
        }
    }

    /// Wraps the registry with the person's stances (ADR 0062), what the record has
    /// proven, their envelope (ADR 0051), and per-capability settings (ADR 0054).
    #[must_use]
    pub fn with_config(
        capabilities: Arc<Vec<Arc<dyn Capability>>>,
        stances: Vec<(String, Stance)>,
        proven: std::collections::HashSet<String>,
        envelope: crate::application::AutonomyEnvelope,
        settings: std::collections::HashMap<String, CapabilitySettings>,
    ) -> Self {
        Self {
            capabilities,
            stances: stances.into_iter().collect(),
            proven,
            envelope,
            settings,
        }
    }

    /// The person's stance on a tool, or its band's default (ADR 0062).
    fn stance_of(&self, info: &CapabilityInfo) -> Stance {
        self.stances
            .get(info.id)
            .copied()
            .unwrap_or_else(|| default_stance(info.reversibility))
    }

    /// Whether a capability may appear at all — `off` is visible but never offered.
    fn is_enabled(&self, info: &CapabilityInfo) -> bool {
        self.stance_of(info) != Stance::Off
    }

    /// The stored settings for a capability (empty if none set).
    fn settings_for(&self, id: &str) -> CapabilitySettings {
        self.settings.get(id).cloned().unwrap_or_default()
    }
}

/// The band's default stance, where the person has said nothing (ADR 0062).
///
/// A read reports the world and runs; something reversible asks; the un-undoable — and
/// every unproven MCP tool, which is classed with it because a server's self-report is not
/// evidence — is blocked until somebody moves it. Deny-by-default lives here.
#[must_use]
pub const fn default_stance(band: Reversibility) -> Stance {
    match band {
        Reversibility::Observe => Stance::Auto,
        // Reversible on-device runs; outward-but-reversible asks, because reach is the
        // person's second dial and `auto` would take it out of their hands by default.
        Reversibility::Reversible => Stance::Auto,
        Reversibility::OutwardReversible => Stance::Ask,
        Reversibility::Irreversible => Stance::Off,
    }
}

/// Whether every setting a capability declares has a value — i.e. it is set up.
/// Whether every setting a skill actually needs has a value.
///
/// Public because the interface asks the same question, and had been answering it with its
/// own copy — which then did not learn that settings can be optional, so a skill stayed
/// "needs setup" in the console after policy had already decided it was ready. One rule,
/// one implementation.
#[must_use]
pub fn settings_complete(info: &CapabilityInfo, settings: &CapabilitySettings) -> bool {
    info.settings
        .iter()
        .filter(|s| !s.optional)
        .all(|s| settings.get(s.key).is_some_and(|v| !v.trim().is_empty()))
}

/// The deterministic classifier (ADR 0051's boundary, ADR 0062's mechanism): one stance,
/// the band, the record, and the envelope — never the model.
///
/// The whole ladder:
///
/// - **`off`** blocks, whoever set it and whyever.
/// - **`ask`** confirms each use — and **graduates**: a tool the record has proven (enough
///   read-back confirmed changes, counted in code) acts on its own while the envelope
///   allows consequential actions. Narrow the envelope and every graduate asks again.
/// - **`auto`** acts — narrowed to confirm for an external read when the person has kept
///   off-device actions in hand, exactly as before.
///
/// What graduation can never do: move `off` (a stance the person set is a decision, and the
/// record does not overrule decisions), or lift a tool nobody vetted (an unproven
/// irreversible-band tool has no `ask` to graduate from — its default is `off`).
fn classify(
    info: &CapabilityInfo,
    env: &crate::application::AutonomyEnvelope,
    stance: Stance,
    proven: bool,
) -> Decision {
    match stance {
        Stance::Off => Decision::Block,
        Stance::Ask => {
            if proven && env.auto_consequential {
                Decision::Act
            } else {
                Decision::Confirm
            }
        }
        Stance::Auto => {
            if info.reaches_external && !env.auto_external {
                Decision::Confirm
            } else {
                Decision::Act
            }
        }
    }
}

/// Whether a skill may run on its own this turn — exactly when the deterministic
/// [`classify`] verdict is [`Act`](Decision::Act).
fn may_run_autonomously(
    info: &CapabilityInfo,
    env: &crate::application::AutonomyEnvelope,
    stance: Stance,
    proven: bool,
) -> bool {
    classify(info, env, stance, proven) == Decision::Act
}

impl CapabilityRunner for RegistryRunner {
    fn available(&self) -> Vec<crate::application::CapabilitySpec> {
        self.capabilities
            .iter()
            .map(|c| {
                let info = c.info();
                crate::application::CapabilitySpec {
                    id: info.id.to_owned(),
                    description: info.description.to_owned(),
                    // Usable only if the code is ready, the person has it enabled, AND
                    // every required setting has a value (ADR 0054).
                    configured: info.configured
                        && self.is_enabled(&info)
                        && settings_complete(&info, &self.settings_for(info.id)),
                    reversibility: info.reversibility,
                    autonomous: may_run_autonomously(
                        &info,
                        &self.envelope,
                        self.stance_of(&info),
                        self.proven.contains(info.id),
                    ),
                    // Built-ins describe their inputs in the prompt, not a schema.
                    input_schema: info.input_schema.map(str::to_owned),
                }
            })
            .collect()
    }

    fn decision(&self, id: &str) -> Option<Decision> {
        self.capabilities
            .iter()
            .find(|c| c.info().id == id)
            .map(|c| {
                let info = c.info();
                classify(
                    &info,
                    &self.envelope,
                    self.stance_of(&info),
                    self.proven.contains(id),
                )
            })
    }

    fn run(&self, id: &str, input_json: &str) -> Result<String, String> {
        let cap = self
            .capabilities
            .iter()
            .find(|c| c.info().id == id)
            .ok_or_else(|| format!("no such skill '{id}'"))?;

        // Deny-by-default on the irreversible band (ADR 0051): the un-undoable is
        // blocked outright — never run, even on an explicit request — until the
        // person opens it per capability. Once opened it reaches this path only via
        // an explicit confirmation. The failure mode is "it refused," never "it did
        // something permanent." The classifier owns which band is blocked.
        let info = cap.info();
        if classify(
            &info,
            &self.envelope,
            self.stance_of(&info),
            self.proven.contains(id),
        ) == Decision::Block
        {
            // One state (`off`), worded by why it is off: the un-undoable was never
            // allowed, anything else the person turned off (ADR 0062).
            return Err(if info.reversibility == Reversibility::Irreversible {
                format!(
                    "the '{id}' skill can't be undone, so Endora won't run it on its own — \
                     it stays blocked until you allow it for this skill"
                )
            } else {
                format!("the '{id}' skill is turned off")
            });
        }
        // Data-loss tripwire: for a skill that leaves the device, refuse to send a
        // request that appears to carry a secret (ADR 0051). Fail closed.
        if cap.info().reaches_external {
            if let Some(kind) = scan_outbound_secret(input_json) {
                return Err(format!(
                    "refusing to send this to '{id}' — the request looks like it contains {kind}"
                ));
            }
        }
        let mut input: Value = serde_json::from_str(input_json.trim())
            .or_else(|_| Ok::<Value, serde_json::Error>(json!({})))
            .unwrap_or_else(|_| json!({}));
        // Query minimization: strip personal identifiers (email addresses) from a
        // request before it leaves the device (ADR 0051).
        if cap.info().reaches_external {
            redact_pii_in_value(&mut input);
        }
        let out = cap
            .invoke(&input, &self.settings_for(id))
            .map_err(|e| e.to_string())?;
        // Hand the butler readable text, not raw JSON — small models relay it far
        // more reliably (and won't miss headlines buried in a JSON array).
        Ok(cap.summarize(&out))
    }
}

// ---- MCP host: transport port, adapter, and the composite runner ----------
//
// An MCP server is a *source* of catalog tools (ADR 0054). Because a tool's id and
// description are discovered at runtime — not the `&'static` metadata the built-in
// `Capability` trait carries — the adapter implements the application-layer
// `CapabilityRunner` directly (whose `CapabilitySpec` is owned), rather than the
// built-in `Capability` trait. The application still speaks only to one
// `CapabilityRunner`; it never learns a tool came from MCP.

/// A tool exposed by a connected MCP server, discovered at connect time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpToolInfo {
    /// The tool's own name on its server (un-namespaced), e.g. `"create_event"`.
    pub name: String,
    /// The one-line description the server advertises for it.
    pub description: String,
    /// The tool's JSON-Schema for its input (`inputSchema` from `tools/list`), if the
    /// server provided one. The model needs this to know *how* to call the tool — the
    /// field names and which are required — otherwise it only knows the tool exists.
    pub input_schema: Option<serde_json::Value>,
}

/// One thing an MCP server says it can be read for (`resources/list`).
///
/// The standard half of the protocol Endora did not speak. A tool is something to *do*; a
/// resource is something to *look at*, which is what the watch loop, the transition log and
/// notions are all made of. Supporting it means a third-party integration can feed those
/// without a line of Rust in this repository (ADR 0058).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpResource {
    /// How the server addresses it. Opaque to Endora, and the only required field: a
    /// resource nobody can name cannot be read, so it is not a resource.
    pub uri: String,
    /// A human name for it, if the server gave one.
    pub name: String,
    /// The one-line description the server advertises, if any.
    pub description: String,
}

/// The boundary to a single MCP server (ADR 0054). The concrete transport — a local
/// stdio subprocess, or a networked HTTP/SSE connection — lives behind this port, so
/// neither the adapter nor the policy layer ever speaks the protocol. Synchronous to
/// match [`CapabilityRunner::run`]; an async transport bridges to this at its edge.
pub trait McpClient: Send + Sync {
    /// Lists the server's tools (its handshake + `tools/list`).
    ///
    /// # Errors
    /// A human-readable message if the server can't be reached or replies badly.
    fn list_tools(&self) -> Result<Vec<McpToolInfo>, String>;

    /// Calls one tool by its (un-namespaced) name with JSON input, returning its
    /// result as text.
    ///
    /// # Errors
    /// A human-readable message if the call fails.
    fn call(&self, tool: &str, input_json: &str) -> Result<String, String>;

    /// What the server offers to be read (`resources/list`).
    ///
    /// **An empty list is the ordinary answer.** Most MCP servers expose tools only, and a
    /// server that does not implement the method at all must read as "nothing to watch"
    /// rather than as a fault — otherwise every tools-only server would start reporting
    /// trouble it does not have.
    ///
    /// # Errors
    /// A human-readable message only if the server could not be reached at all.
    fn list_resources(&self) -> Result<Vec<McpResource>, String> {
        Ok(Vec::new())
    }

    /// Reads one resource by uri, returning its text.
    ///
    /// Unlike listing, a failure here is a real one: something was named and could not be
    /// fetched, which is exactly what a watch loop exists to notice.
    ///
    /// # Errors
    /// A human-readable message if the resource cannot be read.
    fn read_resource(&self, _uri: &str) -> Result<String, String> {
        Err("this server does not offer resources".to_owned())
    }
}

/// One connected server: its name, its transport, and the tools it advertised.
struct McpConnection {
    server: String,
    transport: Box<dyn McpClient>,
    tools: Vec<McpToolInfo>,
    /// The tool the person nominated as this server's state reader (ADR 0054). Empty
    /// when nobody has said, which means no read-back — the honest default.
    reader_tool: String,
}

/// A [`CapabilityRunner`] backed by connected **MCP servers** (ADR 0054). Each
/// server's tools appear in the catalog **namespaced** as `server.tool`, so two
/// servers can never collide on a name.
///
/// Safety posture (this slice): MCP tools are **never autonomous**, and their policy
/// [`Decision`] is [`Block`](Decision::Block) — deny-by-default, treated exactly as
/// the unclassified/irreversible band (ADR 0051). The butler can *see* a tool and
/// propose it, but policy refuses to run it until a later slice classifies tools and
/// lets the person open specific ones. [`run`](Self::run) itself routes faithfully to
/// the transport — its contract is that it is only ever called once policy cleared.
pub struct McpRunner {
    connections: Vec<McpConnection>,
}

impl McpRunner {
    /// Whether this tool is the **state reader** for its server — the one the person
    /// nominated (ADR 0054).
    ///
    /// The nomination comes from the person, never from the server: a server announcing
    /// "I only read" is not evidence of anything, and policy must not take an unvetted
    /// third party's word (ADR 0051). A server with no nomination has no reader, and
    /// everything on it stays deny-by-default.
    fn is_state_reader(&self, id: &str) -> bool {
        let Some((server, tool)) = id.split_once('.') else {
            return false;
        };
        self.connections
            .iter()
            .any(|c| c.server == server && !c.reader_tool.is_empty() && c.reader_tool == tool)
    }

    /// Connects to each `(server_name, transport)`, discovering its tools up front. A
    /// server whose `list_tools` fails is **skipped** — it contributes no tools rather
    /// than failing the whole runner, so one unhealthy server can't take down the host
    /// (ADR 0054).
    #[must_use]
    pub fn connect(servers: Vec<(String, Box<dyn McpClient>)>) -> Self {
        Self::connect_with_readers(
            servers
                .into_iter()
                .map(|(server, transport)| (server, transport, String::new()))
                .collect(),
        )
    }

    /// Connects, carrying each server's nominated **state reader** (ADR 0054) — the tool
    /// whose result is an observation and through which that server's actions are
    /// verified. An empty nomination means no read-back for that server.
    #[must_use]
    pub fn connect_with_readers(servers: Vec<(String, Box<dyn McpClient>, String)>) -> Self {
        let connections = servers
            .into_iter()
            .filter_map(
                |(server, transport, reader_tool)| match transport.list_tools() {
                    Ok(tools) => Some(McpConnection {
                        server,
                        transport,
                        reader_tool,
                        tools,
                    }),
                    Err(_) => None,
                },
            )
            .collect();
        Self { connections }
    }

    /// Resolves a namespaced `server.tool` id to its connection and tool.
    fn find(&self, id: &str) -> Option<(&McpConnection, &McpToolInfo)> {
        let (server, tool) = id.split_once('.')?;
        let conn = self.connections.iter().find(|c| c.server == server)?;
        let info = conn.tools.iter().find(|t| t.name == tool)?;
        Some((conn, info))
    }
}

/// Renders an MCP tool's JSON-Schema input into a compact one-line hint the model can
/// act on — the field names, their types, and which are required — without dumping the
/// whole schema into the prompt. `None` when there are no properties to describe (a
/// no-argument tool needs no hint).
fn compact_input_hint(schema: &serde_json::Value) -> Option<String> {
    let props = schema.get("properties")?.as_object()?;
    if props.is_empty() {
        return None;
    }
    let required: std::collections::HashSet<&str> = schema
        .get("required")
        .and_then(serde_json::Value::as_array)
        .map(|a| a.iter().filter_map(serde_json::Value::as_str).collect())
        .unwrap_or_default();
    // **Required fields only, and the rest counted.**
    //
    // This listed every parameter a server declared. A search tool with nine of them —
    // count, country, freshness, goggles, offset, result_filter, safesearch, extra_snippets,
    // query — read to a small model as nine things to collect, and it turned round and asked
    // the person for them:
    //
    //     To provide you with relevant news, I'll need some more details such as the count
    //     of results you'd like to see, your preferred country… what level of freshness you
    //     require.
    //
    // That is somebody else's API surface, put to the person as a question. Optional means
    // the server has a default, so naming them invites a weak model to gather what it was
    // never required to have. The real schema still travels structurally, so nothing is
    // hidden from a model that wants to set them — this is only what the prose says.
    let mut fields: Vec<String> = props
        .iter()
        .filter(|(name, _)| required.contains(name.as_str()))
        .map(
            |(name, spec)| match spec.get("type").and_then(serde_json::Value::as_str) {
                Some(ty) => format!("{name} ({ty})"),
                None => name.clone(),
            },
        )
        .collect();
    fields.sort(); // stable order → a deterministic prompt
    let optional = props.len() - fields.len();
    if fields.is_empty() {
        // Everything is optional: say so, rather than listing a menu.
        return Some(format!(
            "call it with no arguments, or set any of its {optional} options"
        ));
    }
    let rest = if optional == 0 {
        String::new()
    } else {
        format!(" ({optional} more are optional — leave them out)")
    };
    Some(format!("needs: {}{rest}", fields.join(", ")))
}

/// How many resources one server may contribute to a single watch pass.
///
/// The watch loop runs every two minutes, and reading a resource is a round trip. A server
/// advertising thousands would otherwise turn a background tick into a stampede against
/// somebody else's service. Bounded by the house rather than by uptime, the same argument
/// that made standing trouble safe to store.
const MOST_RESOURCES_READ_PER_SERVER: usize = 200;

impl CapabilityRunner for McpRunner {
    /// Every resource each connected server offers, read (ADR 0058).
    ///
    /// This is the whole point of speaking the resources half of MCP: what the watch loop
    /// sees is no longer limited to integrations written in Rust in this repository. A
    /// third-party server that publishes resources feeds trouble detection, the transition
    /// log and notions with no code here at all.
    ///
    /// Two failures are swallowed on purpose, and they are different from each other. A
    /// server with **no resources** is the ordinary case — most expose tools only — and must
    /// never look like trouble, because absence is exactly what the watch loop raises. A
    /// single **unreadable** resource is dropped so that one bad entry cannot cost the pass
    /// every other reading it had; the read error is still an error at the transport, where a
    /// caller asking for that one thing will see it.
    fn current_states(&self) -> Vec<(String, String)> {
        self.connections
            .iter()
            .flat_map(|c| {
                c.transport
                    .list_resources()
                    .unwrap_or_default()
                    .into_iter()
                    .take(MOST_RESOURCES_READ_PER_SERVER)
                    .filter_map(|r| {
                        // Keyed by **server and uri**, not uri alone. Two servers may publish
                        // the same resource name, and a flat key would have one silently
                        // overwrite the other — invisible, because both would look present.
                        // It also gives every consumer downstream a name that says where a
                        // fact came from, which is what makes one fact source usable by three
                        // different things.
                        c.transport
                            .read_resource(&r.uri)
                            .ok()
                            .map(|text| (format!("{}::{}", c.server, r.uri), text))
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    fn available(&self) -> Vec<crate::application::CapabilitySpec> {
        // The ids Endora treats as state readers, resolved once rather than per tool.
        let reader_ids: std::collections::HashSet<String> = self
            .connections
            .iter()
            .flat_map(|c| {
                c.tools
                    .iter()
                    .map(move |t| format!("{}.{}", c.server, t.name))
            })
            .filter(|id| self.is_state_reader(id))
            .collect();
        let reader_ids = &reader_ids;
        self.connections
            .iter()
            .flat_map(|c| {
                c.tools.iter().map(move |t| {
                    // Fold the tool's input shape into the description so the model
                    // learns HOW to call it (field names + which are required), not
                    // just that it exists. Built-in skills get input examples in the
                    // prompt; MCP tools only have this schema to go on.
                    let description = match t.input_schema.as_ref().and_then(compact_input_hint) {
                        Some(hint) => format!("{} — {hint}", t.description),
                        None => t.description.clone(),
                    };
                    // A tool Endora itself designates as that server's state reader is
                    // a READ: it reports the world, it does not act on it. Left in the
                    // irreversible band it was handed `[unverified] this is what the
                    // tool reported about its own work` — telling the model not to trust
                    // the very reading that is supposed to be the confirmation, which is
                    // incoherent and was observed live. A read has no work of its own to
                    // be unverified about (ADR 0053/0037).
                    let reads_state =
                        reader_ids.contains(format!("{}.{}", c.server, t.name).as_str());
                    crate::application::CapabilitySpec {
                        id: format!("{}.{}", c.server, t.name),
                        description,
                        configured: true,
                        // Deny-by-default everywhere else: the server tells us nothing
                        // about whether a tool reads or actuates, so its result is a
                        // receipt, not evidence (ADR 0053).
                        autonomous: reads_state,
                        reversibility: if reads_state {
                            Reversibility::Observe
                        } else {
                            Reversibility::Irreversible
                        },
                        // The real schema travels structurally too (as text), so the
                        // model layer can offer this tool through native tool-calling.
                        input_schema: t.input_schema.as_ref().map(ToString::to_string),
                    }
                })
            })
            .collect()
    }

    fn decision(&self, id: &str) -> Option<Decision> {
        // Only answer for tools we host; an unknown id is `None` so a composite can
        // consult another source. Deny-by-default, except the state reader Endora uses
        // to verify actions — a read is in the `Observe` band and may run on its own, so
        // the read-back still works when the person has opened the actions but not the
        // reader.
        self.find(id).map(|_| {
            if self.is_state_reader(id) {
                Reversibility::Observe.default_decision()
            } else {
                Reversibility::Irreversible.default_decision()
            }
        })
    }

    /// Home Assistant hosts both actuators (`Hass*`) and a state reader
    /// (`GetLiveContext`) on the same server, so an action can be checked against the
    /// world it just claimed to change (ADR 0053). One mapping for the whole
    /// integration — every `Hass*` action verifies through the same reader.
    ///
    /// Servers Endora knows nothing about return `None`, so their results stay marked
    /// unverified rather than being vouched for.
    fn verifier(&self, id: &str) -> Option<String> {
        let (server, _) = id.split_once('.')?;
        // Whatever the person nominated for THIS server (ADR 0054). No integration is
        // named here: a calendar or filesystem server gets read-back on exactly the same
        // terms as Home Assistant, as soon as someone says which of its tools reads.
        let conn = self
            .connections
            .iter()
            .find(|c| c.server == server && !c.reader_tool.is_empty())?;
        let reader = format!("{server}.{}", conn.reader_tool);
        // Only if that server really exposes it, and never verify a read with itself.
        (id != reader && self.find(&reader).is_some()).then_some(reader)
    }

    fn read_back_input(&self, action_id: &str, action_input: &str) -> String {
        const WHOLE: &str = "{}";
        let Some(reader_id) = self.verifier(action_id) else {
            return WHOLE.to_owned();
        };
        let Some((_, reader)) = self.find(&reader_id) else {
            return WHOLE.to_owned();
        };
        // Whatever targeting arguments the READER also accepts, taken from the action's
        // own input. Schema against schema — nothing here knows what an "area" is.
        let (Some(schema), Ok(args)) = (
            reader.input_schema.as_ref(),
            serde_json::from_str::<Value>(action_input),
        ) else {
            return WHOLE.to_owned();
        };
        let (Some(props), Some(args)) = (
            schema.get("properties").and_then(Value::as_object),
            args.as_object(),
        ) else {
            return WHOLE.to_owned();
        };
        // Location narrows the reading; a KIND filter must not. Asked to turn off the
        // kitchen main — which is a `switch` — the model sent `domain: ["light"]`. If
        // the read-back inherited that, it would show the kitchen's lights and hide the
        // switch: exactly the entity that explains the failure. After an action that did
        // not land, "what is actually there" is the whole point of looking (ADR 0053).
        //
        // The split is by JSON shape rather than by field name, so it needs no knowledge
        // of any server: a scalar (`name`, `area`, `floor`) points at something, while an
        // array (`domain`, `device_class`) restricts which kinds count.
        let scoped: serde_json::Map<String, Value> = args
            .iter()
            .filter(|(key, value)| {
                props.contains_key(*key) && !value.is_null() && !value.is_array()
            })
            .map(|(key, value)| ((*key).clone(), (*value).clone()))
            .collect();
        if scoped.is_empty() {
            return WHOLE.to_owned();
        }
        Value::Object(scoped).to_string()
    }

    fn run(&self, id: &str, input_json: &str) -> Result<String, String> {
        let (conn, tool) = self
            .find(id)
            .ok_or_else(|| format!("no such MCP tool '{id}'"))?;
        // Weak models pass a scalar where the schema wants an array (e.g. HA's
        // HassTurnOn wants domain:["light"] but the model sends "light"). Coerce the
        // arguments to the tool's schema before the call so a single-value slip
        // doesn't fail validation.
        let args = coerce_args_to_schema(input_json, tool.input_schema.as_ref());
        conn.transport.call(&tool.name, &args)
    }
}

/// Passes one method of [`CapabilityRunner`](crate::application::CapabilityRunner) straight
/// through to a wrapped runner. See [`forwards_to_inner!`].
macro_rules! forward_one_to_inner {
    ($field:ident, about_the_person) => {
        fn about_the_person(&self) -> Vec<String> {
            self.$field.about_the_person()
        }
    };
    ($field:ident, current_states) => {
        fn current_states(&self) -> Vec<(String, String)> {
            self.$field.current_states()
        }
    };
    ($field:ident, decision) => {
        fn decision(&self, id: &str) -> Option<Decision> {
            self.$field.decision(id)
        }
    };
    ($field:ident, verifier) => {
        fn verifier(&self, id: &str) -> Option<String> {
            self.$field.verifier(id)
        }
    };
    ($field:ident, read_back_input) => {
        fn read_back_input(&self, action_id: &str, action_input: &str) -> String {
            self.$field.read_back_input(action_id, action_input)
        }
    };
}

/// Declares which of the port's **defaultable** methods a decorator simply passes along.
///
/// Runners are layered, and every method a wrapper does not answer itself has to be handed
/// down. Five of the seven have a default, and a default that returns nothing is
/// **indistinguishable from a service having nothing to say** — so forgetting one fails
/// silently and looks exactly like working. Two were forgotten this way: presence never
/// reached a turn and neither did the facts behind an answer, with every unit test passing
/// because they exercised the runner that answers rather than the stack production builds.
///
/// Two more were latent at the time of writing: `AliasRunner` and `OpenerRunner` never
/// passed presence or states along, and got away with it only because the runner *above*
/// them happened to answer first. Moving them in the stack would have broken it.
///
/// Writing it out per decorator is what made that possible. Here, adding a method to the
/// port means adding one arm to [`forward_one_to_inner!`], and every decorator that lists it
/// forwards it correctly by construction. What a decorator genuinely overrides it still
/// writes by hand, right next to this — so the difference between "changes this" and "passes
/// this along" is visible in one place.
macro_rules! forwards_to_inner {
    ($field:ident : $($method:ident),+ $(,)?) => {
        $( forward_one_to_inner!($field, $method); )+
    };
}

/// Reduces a call's arguments to what they **mean**, so that two spellings of one call
/// are recognised as one call (ADR 0053).
///
/// The turn loop refuses to run the same tool with the same input twice, and was being
/// beaten by punctuation and key order. Observed live in one morning briefing: two
/// identical requests for the whole house, and before them two identical failing attempts
/// to get weather out of the smart home — four rounds spent, of which two were free, on a
/// turn whose answer then had no room left to be any good.
///
/// Key order is not incidental here: the same model emits
/// `{"area":"","domain":["light"],"name":""}` and
/// `{"domain":["light"],"name":"","area":""}` for the same intent, run to run.
///
/// Arguments that will not parse fall back to the raw text — exactly what the guard
/// compared before, so nothing gets worse.
#[must_use]
pub fn same_call_as(input_json: &str) -> String {
    // `serde_json`'s map is ordered by key unless `preserve_order` is enabled, which it is
    // not here, so re-serialising a parsed value is already canonical.
    serde_json::from_str::<Value>(input_json)
        .as_ref()
        .map_or_else(|_| input_json.trim().to_owned(), Value::to_string)
}

/// Nudges tool arguments toward the tool's input schema for the common small-model
/// slip of passing a scalar where an array is wanted: for each top-level property the
/// schema types as `array`, a non-array value is wrapped in a one-element array. Only
/// that one coercion — everything else is passed through untouched, and any parse
/// failure returns the input unchanged (the server still validates).
fn coerce_args_to_schema(input_json: &str, schema: Option<&serde_json::Value>) -> String {
    let Some(schema) = schema else {
        return input_json.to_owned();
    };
    let Some(props) = schema.get("properties").and_then(|p| p.as_object()) else {
        return input_json.to_owned();
    };
    let Ok(mut args) = serde_json::from_str::<serde_json::Value>(input_json) else {
        return input_json.to_owned();
    };
    let Some(obj) = args.as_object_mut() else {
        return input_json.to_owned();
    };
    for (key, val) in obj.iter_mut() {
        let wants_array = props
            .get(key)
            .and_then(|s| s.get("type"))
            .and_then(serde_json::Value::as_str)
            == Some("array");
        if wants_array && !val.is_array() && !val.is_null() {
            *val = serde_json::Value::Array(vec![val.take()]);
        }
        // Drop values the schema says are not allowed, rather than letting the server
        // reject the whole call. Observed live: `device_class: ["light"]` — not one of
        // that field's permitted values — failed the entire turn with a validation
        // error, so a light nobody could name stayed on.
        if let Some(allowed) = permitted_values(props.get(key)) {
            match val {
                serde_json::Value::Array(items) => {
                    items.retain(|i| i.as_str().is_some_and(|s| allowed.iter().any(|a| a == s)));
                }
                serde_json::Value::String(sv) if !allowed.iter().any(|a| a == sv) => {
                    *val = serde_json::Value::Null;
                }
                _ => {}
            }
        }
    }
    // Did the model TRY to aim at something? Recorded before the empties are dropped,
    // because that is the only moment the difference is visible.
    let aimed_at_something = obj.iter().any(|(_, v)| is_targeting_attempt(v));
    // A kind filter that merely repeats another one is a duplication, not a narrowing.
    // Live: HassTurnOff{area:"kitchen", domain:["switch"], device_class:["switch"]} —
    // the same word twice. `Kitchen Main Light` is domain `switch` with no matching
    // device_class, so the pair excluded everything and Home Assistant reported the
    // AREA as unmatched even though it exists.
    //
    // Dropping the duplicate cannot widen what the call can touch: the filter it
    // duplicates is still there, so the blast radius is unchanged. That is the whole
    // reason this is safe to do silently, where dropping a filter in general is not.
    drop_duplicated_kind_filters(obj);
    // An empty value is not a filter, it is noise — and some servers reject it outright
    // (`floor: ""` came back as "invalid slot info", failing the call). Dropping empties
    // also clears anything emptied by the enum check above.
    obj.retain(|_, v| match v {
        serde_json::Value::Null => false,
        serde_json::Value::String(s) => !s.trim().is_empty(),
        serde_json::Value::Array(items) => !items.is_empty(),
        _ => true,
    });
    // Dropping empties is what lets a slightly-malformed call through — but it must not
    // turn a call that aimed at ONE thing into a call that hits everything.
    //
    // Observed live: HassTurnOn{area:null, name:null, domain:["light"]} became
    // {domain:["light"]} and Home Assistant turned on every light in the house. The
    // model plainly meant to name something and sent nulls instead; the honest reading
    // of that is "it failed to say what", not "it meant all of them".
    //
    // So if every targeting value it gave was empty, the call is left alone rather than
    // cleaned into a house-wide one, and the server rejects it as it did before.
    if aimed_at_something && !obj.iter().any(|(_, v)| v.is_string()) {
        return input_json.to_owned();
    }
    args.to_string()
}

/// Whether this value looks like an attempt to aim at something — a name, an area, a
/// floor — as opposed to a kind filter or a setting.
///
/// Scalars point at things and arrays restrict which kinds count, the same split the
/// read-back scoping uses. An empty or null scalar is an attempt that came out blank,
/// which is the case worth noticing.
fn is_targeting_attempt(v: &Value) -> bool {
    v.is_null() || v.is_string()
}

/// Removes an array-valued filter whose values are already covered by another one.
///
/// Only exact duplicates go: `domain: ["switch"]` alongside `device_class: ["switch"]`
/// keeps one and drops the other, while `domain: ["light"]` alongside
/// `device_class: ["outlet"]` is a real narrowing and both stay. Scalars are never
/// touched — they name the target rather than restricting its kind.
///
/// The keeper is chosen by name order so the result is deterministic, and the filter
/// that survives constrains exactly as much as the pair did.
fn drop_duplicated_kind_filters(obj: &mut serde_json::Map<String, serde_json::Value>) {
    let arrays: Vec<(String, Vec<String>)> = obj
        .iter()
        .filter_map(|(k, v)| {
            let items: Vec<String> = v
                .as_array()?
                .iter()
                .filter_map(|i| i.as_str().map(str::to_lowercase))
                .collect();
            (!items.is_empty()).then(|| (k.clone(), items))
        })
        .collect();
    let mut drop: Vec<String> = Vec::new();
    for (i, (key, values)) in arrays.iter().enumerate() {
        let duplicated_earlier = arrays[..i]
            .iter()
            .any(|(other, other_values)| other_values == values && !drop.contains(other));
        if duplicated_earlier {
            drop.push(key.clone());
        }
    }
    for key in drop {
        obj.remove(&key);
    }
}

/// The values a schema permits for a field, from `enum` — directly, or on an array's
/// `items`. `None` when the field is unconstrained, which is the common case.
///
/// Schema-driven on purpose: this is what lets Endora keep a model's slips from failing
/// a whole call without knowing anything about the server it is talking to (ADR 0054).
fn permitted_values(field: Option<&serde_json::Value>) -> Option<Vec<String>> {
    let values = collect_enum(field?);
    (!values.is_empty()).then_some(values)
}

/// Every value an `enum` anywhere in this schema fragment permits.
///
/// A schema says "one of these" in more shapes than one: directly, on an array's `items`,
/// and — the shape that got through — nested inside `anyOf` / `oneOf` / `allOf`, which is
/// how a generated schema usually expresses "a string from this list, or null".
///
/// Looking only at the top two levels missed it, so a value the server would reject was
/// sent anyway. Live: `device_class: ["light"]` — not a device class, and Home Assistant
/// answered `'light' is not one of ['awning', 'blind', 'curtain', …]`, failing the whole
/// call. Endora had the list in the tool's own schema the entire time.
fn collect_enum(field: &serde_json::Value) -> Vec<String> {
    let mut values: Vec<String> = Vec::new();
    if let Some(list) = field.get("enum").and_then(serde_json::Value::as_array) {
        values.extend(
            list.iter()
                .filter_map(|v| v.as_str().map(ToOwned::to_owned)),
        );
    }
    for key in ["items", "anyOf", "oneOf", "allOf", "prefixItems"] {
        match field.get(key) {
            Some(serde_json::Value::Array(branches)) => {
                for branch in branches {
                    values.extend(collect_enum(branch));
                }
            }
            Some(nested) => values.extend(collect_enum(nested)),
            None => {}
        }
    }
    values.sort();
    values.dedup();
    values
}

/// Merges several [`CapabilityRunner`] sources — the built-in registry and any MCP
/// servers — behind the single runner interface (ADR 0054). The application still
/// speaks to one `CapabilityRunner` and never learns a tool's origin. Ids are unique
/// across sources by construction (built-ins carry no dot; MCP tools are
/// `server.tool`), so the first source that lists an id owns it.
pub struct CompositeRunner {
    sources: Vec<Arc<dyn CapabilityRunner + Send + Sync>>,
}

impl CompositeRunner {
    /// Merges the given sources, consulted in order. Sources are [`Arc`] so a
    /// long-lived one (e.g. connected MCP servers) can be shared across turns while a
    /// fresh per-turn source (the config-bound registry runner) sits beside it.
    #[must_use]
    pub fn new(sources: Vec<Arc<dyn CapabilityRunner + Send + Sync>>) -> Self {
        Self { sources }
    }

    /// The source that lists `id` in its catalog, if any.
    fn owner(&self, id: &str) -> Option<&(dyn CapabilityRunner + Send + Sync)> {
        self.sources
            .iter()
            .find(|s| s.available().iter().any(|spec| spec.id == id))
            .map(AsRef::as_ref)
    }
}

impl CapabilityRunner for CompositeRunner {
    fn available(&self) -> Vec<crate::application::CapabilitySpec> {
        self.sources.iter().flat_map(|s| s.available()).collect()
    }

    fn about_the_person(&self) -> Vec<String> {
        self.sources
            .iter()
            .flat_map(|s| s.about_the_person())
            .collect()
    }

    fn current_states(&self) -> Vec<(String, String)> {
        self.sources
            .iter()
            .flat_map(|s| s.current_states())
            .collect()
    }

    fn decision(&self, id: &str) -> Option<Decision> {
        self.owner(id)?.decision(id)
    }

    fn run(&self, id: &str, input_json: &str) -> Result<String, String> {
        match self.owner(id) {
            Some(source) => source.run(id, input_json),
            None => Err(format!("no such skill '{id}'")),
        }
    }

    fn verifier(&self, id: &str) -> Option<String> {
        self.owner(id)?.verifier(id)
    }

    fn read_back_input(&self, action_id: &str, action_input: &str) -> String {
        self.owner(action_id).map_or_else(
            || "{}".to_owned(),
            |o| o.read_back_input(action_id, action_input),
        )
    }
}

/// Retries a **failed** call once with the person's confirmed target aliases applied
/// (ADR 0054).
///
/// Endora asks what a target is really called; the answer used to reach the model as
/// context and nothing more. Measured, that does not work: with `"table" means "Kitchen
/// Table"` in its prompt, the model still sent `name: "table"` and Home Assistant still
/// answered `no_match_reason=NAME`. Grounding is advice, and this model takes advice
/// about a third of the time.
///
/// So the alias becomes deterministic — but only as **recovery**, never as pre-emption:
///
/// - it fires only after a call has already **failed**, so it can never hijack a working
///   one or redirect an action that was about to hit the right thing;
/// - it uses only what the person **confirmed**, which is the authoritative source in
///   ADR 0054's ranking;
/// - it retries **once**;
/// - and it **says so** in the result, so the substitution is visible to the model, to
///   the outcome record, and to the person in the disclosure.
///
/// That last point is what keeps it honest. The model's mistake still happened, is still
/// recorded, and is still measurable — the eval sees the same failure it always did. What
/// changes is that the person's light comes on.
pub struct AliasRunner {
    inner: Arc<dyn CapabilityRunner + Send + Sync>,
    /// `(server, said, means)`, as the person confirmed them.
    aliases: Vec<(String, String, String)>,
}

impl AliasRunner {
    /// Wraps `inner` with the confirmed aliases.
    #[must_use]
    pub fn new(
        inner: Arc<dyn CapabilityRunner + Send + Sync>,
        aliases: Vec<(String, String, String)>,
    ) -> Self {
        Self { inner, aliases }
    }

    /// The input with any confirmed alias applied, or `None` when nothing matched.
    ///
    /// Matches a whole string **value** only — never a fragment, and never a field name.
    /// Replacing inside a value would let `"table"` rewrite `"comfortable"`, and a person
    /// who said what one thing is called has not licensed edits to everything containing
    /// that word.
    fn apply(&self, id: &str, input_json: &str) -> Option<String> {
        let server = id.split_once('.').map(|(s, _)| s)?;
        let mut args: Value = serde_json::from_str(input_json).ok()?;
        let obj = args.as_object_mut()?;
        let mut resolved: Vec<String> = Vec::new();
        for (field, value) in obj.iter_mut() {
            let Some(text) = value.as_str() else { continue };
            let hit = self
                .aliases
                .iter()
                .find(|(srv, said, _)| srv == server && said.eq_ignore_ascii_case(text.trim()));
            if let Some((_, _, means)) = hit {
                *value = Value::String(means.clone());
                resolved.push(field.clone());
            }
        }
        if resolved.is_empty() {
            return None;
        }
        // The person's answer outranks the model's other guesses (ADR 0054's ranking).
        //
        // Observed live, with the alias in place and still failing:
        //   {name: "table light", area: "Living Room", floor: "1"} -> INVALID_FLOOR
        // The kitchen light was put on an invented floor in the wrong room, so the
        // substitution never got as far as being tried. A confirmed name identifies one
        // thing on its own; every other scalar the model supplied is a guess about the
        // same target, and keeping them can only contradict the answer.
        //
        // Kind filters are kept — they restrict which sorts of thing count rather than
        // claiming which one it is, and dropping them would widen the call.
        obj.retain(|field, value| resolved.contains(field) || !value.is_string());
        Some(args.to_string())
    }
}

impl CapabilityRunner for AliasRunner {
    // Only rewrites the arguments of a call; everything else passes through. The
    // first two were missing here and got away with it because the runner above
    // answered them first.
    forwards_to_inner!(inner: about_the_person, current_states, decision, verifier, read_back_input);

    fn available(&self) -> Vec<crate::application::CapabilitySpec> {
        self.inner.available()
    }

    fn run(&self, id: &str, input_json: &str) -> Result<String, String> {
        let first = self.inner.run(id, input_json);
        let Err(original) = first else {
            return first;
        };
        let Some(retry_input) = self.apply(id, input_json) else {
            return Err(original);
        };
        match self.inner.run(id, &retry_input) {
            // Say what was substituted. The model answers from this, the outcome record
            // keeps it, and the person sees it — nothing about the recovery is hidden.
            Ok(out) => Ok(format!(
                "(Endora retried using the name you gave it: {retry_input}. The first \
                 attempt failed.)\n{out}"
            )),
            // The alias did not help either; the original failure is the honest answer.
            Err(_) => Err(original),
        }
    }
}

/// Overlays the person's stances — and what the record has proven — onto the shared MCP
/// runner (ADR 0062), so a tool's word applies without rebuilding the connection.
///
/// An MCP tool's default is `off`: a server's self-report is not evidence, so an unvetted
/// tool is blocked outright. `ask` confirms each use — and graduates to acting on its own
/// when the record has proven the tool AND the person's envelope allows consequential
/// actions: the same two deliberate gates as before, with the record standing where a
/// stored "opened" flag used to.
pub struct OpenerRunner {
    inner: Arc<dyn CapabilityRunner + Send + Sync>,
    stances: std::collections::HashMap<String, Stance>,
    /// Tools with enough read-back confirmed changes (ADR 0062). Derived, never stored.
    proven: std::collections::HashSet<String>,
    /// The envelope's consequential dial — graduation's second gate.
    auto_consequential: bool,
}

impl OpenerRunner {
    /// Overlays `stances` and `proven` onto `inner`.
    #[must_use]
    pub fn new(
        inner: Arc<dyn CapabilityRunner + Send + Sync>,
        stances: std::collections::HashMap<String, Stance>,
        proven: std::collections::HashSet<String>,
        auto_consequential: bool,
    ) -> Self {
        Self {
            inner,
            stances,
            proven,
            auto_consequential,
        }
    }

    /// A tool's stance: the person's word, or its band's default.
    fn stance_of(&self, id: &str, band: Reversibility) -> Stance {
        self.stances
            .get(id)
            .copied()
            .unwrap_or_else(|| default_stance(band))
    }

    /// Ask's graduation (ADR 0062): proven by read-back, and the envelope allows it.
    fn graduates(&self, id: &str) -> bool {
        self.proven.contains(id) && self.auto_consequential
    }
}

impl CapabilityRunner for OpenerRunner {
    // Only widens what may run; it has no opinion on what the services can see.
    forwards_to_inner!(inner: about_the_person, current_states, verifier, read_back_input);

    fn available(&self) -> Vec<crate::application::CapabilitySpec> {
        self.inner
            .available()
            .into_iter()
            .map(|mut spec| {
                match self.stance_of(&spec.id, spec.reversibility) {
                    Stance::Off => spec.autonomous = false,
                    Stance::Ask => spec.autonomous = self.graduates(&spec.id),
                    Stance::Auto => spec.autonomous = true,
                }
                spec
            })
            .collect()
    }

    fn decision(&self, id: &str) -> Option<Decision> {
        // The service below is always asked — its own verdict must never be silently
        // swallowed by an overlay. The stance then narrows or widens it: `off` blocks
        // whatever was said below; `ask` is the person's allow-with-confirm, which is
        // precisely what may override a deny-by-default Block (and graduates when the
        // record and the envelope both say so); `auto` keeps whatever the service said,
        // so a stricter verdict below survives.
        let below = self.inner.decision(id);
        let band = self
            .inner
            .available()
            .into_iter()
            .find(|s| s.id == id)
            .map(|s| s.reversibility);
        let Some(band) = band else {
            return below;
        };
        Some(match self.stance_of(id, band) {
            Stance::Off => Decision::Block,
            Stance::Ask => {
                if self.graduates(id) {
                    Decision::Act
                } else {
                    Decision::Confirm
                }
            }
            Stance::Auto => below.unwrap_or(Decision::Act),
        })
    }

    fn run(&self, id: &str, input_json: &str) -> Result<String, String> {
        // Deny-by-default at the run layer too: `off` never runs, even on a direct call.
        if self.decision(id) == Some(Decision::Block) {
            return Err(format!(
                "'{id}' isn't allowed yet — allow it under Skills first (it will still \
                 confirm every use until it has proven itself)"
            ));
        }
        self.inner.run(id, input_json)
    }
}

/// A wrapper for the butler's **unattended** turns — the heartbeat's check-in, daily
/// brief, and nightly loop — that clamps autonomy to the *reversible* bands.
///
/// The person's levers ([`OpenerRunner`], the autonomy envelope) answer "may Endora do
/// this **when I am here**": opening a tool and widening the envelope together clear an
/// irreversible capability to run inside a chat turn, where the person is present, sees
/// the activity trail, and can say stop. None of that is true at 03:00 while they sleep.
///
/// So an unattended turn gets a narrower catalog: anything above
/// [`Reversibility::Reversible`] loses `autonomous` and its [`Act`](Decision::Act)
/// verdict drops to [`Confirm`](Decision::Confirm) — there is nobody to confirm, so in
/// practice it simply does not run, and the butler is handed a factual tool result
/// saying so. `Observe` and `Reversible` capabilities are untouched, which is what lets
/// the nightly loop still research, draft, and form beliefs (ADR 0051).
///
/// This makes [`run_due_nightly_loop`]'s documented guarantee — "nothing here it could
/// do that it couldn't undo" — true in code rather than only in prose. Before this, that
/// claim held only while the envelope happened to be closed.
///
/// Deny-by-default: a capability whose band cannot be read is assumed to be an actuator,
/// the same rule ADR 0053 applies to verification.
pub struct ReversibleOnlyRunner {
    inner: Arc<dyn CapabilityRunner + Send + Sync>,
}

impl ReversibleOnlyRunner {
    /// Wraps `inner` so only reversible capabilities may act without a person.
    #[must_use]
    pub fn new(inner: Arc<dyn CapabilityRunner + Send + Sync>) -> Self {
        Self { inner }
    }

    /// Whether this capability may run with nobody there — `Observe` and `Reversible`
    /// only. An id with no visible band is treated as an actuator, so it may not.
    fn may_run_unattended(&self, id: &str) -> bool {
        self.inner
            .available()
            .into_iter()
            .find(|s| s.id == id)
            .is_some_and(|s| s.reversibility <= Reversibility::Reversible)
    }
}

impl CapabilityRunner for ReversibleOnlyRunner {
    // Unattended turns narrow what may ACT. Seeing is not acting.
    forwards_to_inner!(inner: about_the_person, current_states, verifier, read_back_input);

    // Clamping what may ACT unattended says nothing about what may be known. Presence
    // matters most on exactly these turns — whether anyone is home is half of whether to
    // speak at all — so dropping it here would have been the worst place to drop it.

    fn available(&self) -> Vec<crate::application::CapabilitySpec> {
        self.inner
            .available()
            .into_iter()
            .map(|mut spec| {
                if spec.reversibility > Reversibility::Reversible {
                    spec.autonomous = false;
                }
                spec
            })
            .collect()
    }

    fn decision(&self, id: &str) -> Option<Decision> {
        match self.inner.decision(id)? {
            // Cleared to act when someone is present, but this is not that: fall back
            // to needing a person, who is by definition not here to give it.
            Decision::Act if !self.may_run_unattended(id) => Some(Decision::Confirm),
            other => Some(other),
        }
    }

    fn run(&self, id: &str, input_json: &str) -> Result<String, String> {
        // Refused at the run layer too, so a direct call can't route around the
        // narrowed catalog above.
        if !self.may_run_unattended(id) {
            return Err(format!(
                "'{id}' can't run unattended — it changes something Endora couldn't take \
                 back, and nobody is here to confirm it"
            ));
        }
        self.inner.run(id, input_json)
    }
}

/// A server's **own** interface, richer than the tool surface it exposes to a model
/// (ADR 0054).
///
/// A tool catalogue is a product decision by whoever wrote the server, and it is often
/// the *voice assistant* view: fuzzy names, no identifiers. The same service usually has
/// an API underneath where things have ids and cannot be mistaken for one another. Where
/// Endora is given that reach, it should use it — and nothing above this port learns
/// which service it is talking to.
///
/// Deliberately three methods. This is not "run arbitrary API calls": it is *see what
/// exists*, and *act on exactly one of them*.
pub trait NativeChannel: Send + Sync {
    /// Everything the server knows about, as `(id, name)` — the unambiguous identifier
    /// and the name a person would say.
    ///
    /// # Errors
    /// A human-readable message if the service cannot be reached.
    fn known(&self) -> Result<Vec<(String, String)>, String>;

    /// The same knowledge rendered as text, so the target search reads it exactly as it
    /// reads any other reading.
    ///
    /// # Errors
    /// A human-readable message if the service cannot be reached.
    fn reading(&self) -> Result<String, String>;

    /// Does what `tool` was trying to do, to exactly one thing, by id. `None` when this
    /// channel cannot express that particular tool — the caller falls back.
    fn act(&self, tool: &str, id: &str) -> Option<Result<String, String>>;

    /// Whether `tool` could actually operate this thing (ADR 0054).
    ///
    /// Direct reach sees everything a service holds, which includes things that are not
    /// controls: diagnostics, configuration entries, connection indicators. Those share
    /// their device's name almost exactly, so they tie with it in a search and turn a
    /// clear request into an ambiguous one.
    ///
    /// True by default — a channel that cannot tell says so by not narrowing anything.
    fn actionable(&self, tool: &str, id: &str) -> bool {
        let _ = (tool, id);
        true
    }

    /// The things this service says belong to **the person**, rather than to the household.
    ///
    /// Decides whether a reading may ever become a belief about them (ADR 0057). Empty is the
    /// honest default and the safe direction: nothing is attributed, rather than the house
    /// being mistaken for the person.
    fn belongs_to_the_person(&self) -> Vec<String> {
        Vec::new()
    }

    /// What this service already has set up, by whatever name it calls each thing.
    ///
    /// The Connect screen offered every service with a **Connect** button and knew nothing
    /// about what was already there, so somebody with a calendar working saw "Connect" beside
    /// it and had nowhere to find out whether it had worked. Offering to do something already
    /// done is worse than not offering — it reads as though the last attempt failed.
    ///
    /// Empty is the honest default: a channel that cannot tell says nothing rather than
    /// implying nothing is connected.
    fn already_connected(&self) -> Vec<String> {
        Vec::new()
    }

    /// Begins connecting a new kind of thing to this service, returning **the service's own
    /// setup form** (ADR 0054).
    ///
    /// Endora does not know what a calendar or a mail account needs, and deliberately does
    /// not learn: the service declares its fields and Endora renders them. A kind of thing
    /// nobody here has heard of works the same as one that ships today.
    ///
    /// `None` from a channel with no notion of setting things up.
    ///
    /// # Errors
    /// Through the inner `Result`, a human-readable message if the service refuses.
    fn begin_setup(&self, _kind: &str) -> Option<Result<crate::domain::SetupForm, String>> {
        None
    }

    /// Answers a form from [`begin_setup`](Self::begin_setup).
    ///
    /// Returns the next form when the service wants more, and `None` inside the `Ok` when it
    /// is finished. **Nothing here is stored.** A credential travels from the person's
    /// keyboard to their own service and is not written down on the way — Endora is passing
    /// a message, not keeping an account.
    ///
    /// # Errors
    /// Through the inner `Result`, a human-readable message if the service refuses.
    fn finish_setup(
        &self,
        _form: &str,
        _answers: &[(String, String)],
    ) -> Option<Result<Option<crate::domain::SetupForm>, String>> {
        None
    }

    /// Reaches the person when they are not looking at Endora (ADR 0056).
    ///
    /// **Nominated, never assumed.** The same shape as naming a server's reader: the person
    /// says which of their own services is how they want to be reached, and Endora uses it.
    /// `None` by default — from a channel that cannot, and from one that can but has not
    /// been told to. Being able to interrupt somebody is a grant, not a capability that
    /// switches itself on.
    ///
    /// This is deliberately **not** a notification feature built into Endora. A push stack
    /// of its own would mean certificates, a service worker, a subscription store and a
    /// third-party relay — all to duplicate something the person already has working on
    /// their phone. Endora does not host the model ([0055](../../docs/adr/0055-the-model-layer.md))
    /// and by the same reasoning it does not host a push service.
    ///
    /// # Errors
    /// Through the inner `Result`, a human-readable message if the service refuses.
    fn notify(&self, _title: &str, _body: &str) -> Option<Result<(), String>> {
        None
    }

    /// Takes something out of the service's own view, or puts it back (ADR 0056).
    ///
    /// The remedy for a thing that has not answered in days and that the person has
    /// confirmed is gone. **Hidden, never deleted**: deleting is destructive, irreversible
    /// from Endora's side, and presumes an opinion about somebody else's house that a
    /// tapped answer does not license. Hiding is the smallest change that makes the
    /// catalogue true again, and it undoes exactly.
    ///
    /// `None` from a channel that cannot do it, which is the default.
    ///
    /// # Errors
    /// Through the inner `Result`, a human-readable message if the service refuses.
    fn hide(
        &self,
        _name: &str,
        _hidden: bool,
    ) -> Option<Result<crate::domain::ConfigWrite, String>> {
        None
    }

    /// Teaches the service that `alias` is another name for the thing it currently calls
    /// `name`, so the service itself resolves it from then on — for every client, not
    /// only for Endora (ADR 0054).
    ///
    /// `None` from a channel that cannot be taught, which is the default: seeing and
    /// acting are a much smaller grant than editing, and a channel earns the third
    /// separately from the first two.
    ///
    /// Returns the **write**, not a sentence — carrying what was changed and what it was
    /// before, so the edit can be logged and put back (ADR 0054). A channel that reports
    /// only that it succeeded has made a change nobody can reverse.
    ///
    /// # Errors
    /// Through the inner `Result`, a human-readable message if the service refuses.
    fn teach(&self, name: &str, alias: &str) -> Option<Result<crate::domain::ConfigWrite, String>> {
        let _ = (name, alias);
        None
    }

    /// What this service says is true right now, as `(name, state)`.
    ///
    /// The facts an answer about state should agree with. A service that cannot be asked
    /// says nothing, and nothing downstream changes.
    ///
    /// # Errors
    /// A human-readable message if the service cannot be reached.
    fn states(&self) -> Result<Vec<(String, String)>, String> {
        Ok(Vec::new())
    }

    /// What this service can say about the person **right now**, in one short line.
    ///
    /// `None` — the default — from a service that knows nothing about them. A smart home
    /// knows whether they are in it; a calendar would know whether they are busy.
    fn about_the_person(&self) -> Option<String> {
        None
    }

    /// A reason this call cannot do anything, so it is refused rather than sent
    /// (ADR 0054).
    ///
    /// `None` — the default — means the channel has nothing to say and the call goes out.
    ///
    /// The point is not politeness. A call that quietly does nothing is the worst kind of
    /// failure: it reports success, changes nothing, and leaves the person and the record
    /// disagreeing. Turning it into an outright refusal is also what makes it derivable as
    /// a finding later.
    fn refuse(&self, tool: &str, input_json: &str) -> Option<String> {
        let _ = (tool, input_json);
        None
    }

    /// Narrows a call that already names exactly one thing, before it is sent
    /// (ADR 0054).
    ///
    /// `None` when there is nothing to narrow, which is the default and the common case.
    ///
    /// This is the one place the channel acts **before** a failure rather than after, and
    /// it is allowed for a single reason: it can only ever make a call hit *less*. Live,
    /// a request for one light arrived as
    /// `{entity_id: "light.kitchen_table", area: "kitchen"}` — an exact identifier and a
    /// whole room. The service matched the room, switched off both kitchen lights, and
    /// reported success, so every guard that watches the failure path stayed silent.
    fn tighten(&self, input_json: &str) -> Option<String> {
        let _ = input_json;
        None
    }

    /// The words this service uses as **categories** — the sorts of thing it has, as
    /// opposed to the names of particular things (ADR 0054).
    ///
    /// Empty by default and empty for any service Endora cannot ask, in which case
    /// nothing downstream changes: guessing at what counts as a category would make an
    /// ordinary `domain: ["light"]` pollute every search.
    ///
    /// # Errors
    /// A human-readable message if the service cannot be reached.
    fn categories(&self) -> Result<Vec<String>, String> {
        Ok(Vec::new())
    }

    /// Makes **one thing that stands for many** — so a request meaning "all of them" can
    /// be an ordinary single-target action (ADR 0054).
    ///
    /// This is the general answer to a request no amount of aiming can express. "Turn off
    /// all the lights" produces a call aimed at nothing, which is indistinguishable from a
    /// model that failed to say what it meant — so it is refused, correctly, and the
    /// person cannot have the thing they asked for.
    ///
    /// Rather than teaching Endora to fan out across many ids at action time — the one
    /// move every guard here exists to prevent — the service is asked to hold a collection
    /// once. After that there is nothing special about it: it is a thing with a name and
    /// an id, hit exactly like any other.
    ///
    /// `None` from a service that cannot hold one, which is the default.
    ///
    /// # Errors
    /// Through the inner `Result`, a human-readable message if the service refuses.
    fn collect(
        &self,
        name: &str,
        ids: &[String],
    ) -> Option<Result<crate::domain::ConfigWrite, String>> {
        let _ = (name, ids);
        None
    }

    /// Takes a name away again — the other half of [`teach`](Self::teach), so a name can
    /// be untold and not only added (ADR 0054).
    ///
    /// # Errors
    /// Through the inner `Result`, a human-readable message if the service refuses.
    fn forget(
        &self,
        name: &str,
        alias: &str,
    ) -> Option<Result<crate::domain::ConfigWrite, String>> {
        let _ = (name, alias);
        None
    }

    /// Puts a change back exactly as it was, from the record of it.
    ///
    /// # Errors
    /// Through the inner `Result`, a human-readable message if the service refuses.
    fn undo(&self, write: &crate::domain::ConfigWrite) -> Option<Result<String, String>> {
        let _ = write;
        None
    }
}

/// Searches a server's own reading for the target a call failed to name, and retries when
/// exactly one thing matches (ADR 0054).
///
/// A failed action already causes Endora to read the state back (ADR 0053), so the list of
/// names that really exist is in hand at the moment it is most useful. Until now that
/// whole reading was handed to the model, which then had to find one line in it and copy
/// it exactly. Measured across fourteen consecutive attempts at a light called
/// `Kitchen Table` — with `names: Kitchen Table` in the reading every time — it never once
/// sent that string. It permuted the *arguments* instead: `domain` flipped between `light`
/// and `switch`, an invented floor, the noun moved into `device_class`.
///
/// So the search runs here, in code:
///
/// - **always**, a shortlist of resembling names replaces "here is the whole house", so
///   the model copies from three lines instead of searching five kilobytes;
/// - **only when exactly one** candidate contains every word the call was aiming at, the
///   call is retried against it. Two plausible names is a guess, and a guess that actuates
///   something is what ADR 0051 exists to prevent.
///
/// Recovery-only, like [`AliasRunner`] and for the same reasons: it cannot hijack a
/// working call, it is bounded, and it says what it did so the model, the outcome record
/// and the person all see the substitution.
///
/// It never widens a call. Kind filters are kept; only scalars the real name already
/// contains are dropped, because `area: "kitchen"` adds nothing to `name: "Kitchen Table"`.
pub struct TargetSearchRunner {
    inner: Arc<dyn CapabilityRunner + Send + Sync>,
    /// Direct reach into a server, by server name (ADR 0054). Where one exists, it is
    /// both the better reading — everything, not only what the tool surface exposes —
    /// and the better way to act, because an id cannot be mis-matched.
    channels: Vec<(String, Arc<dyn NativeChannel>)>,
}

/// How many places a real name may be tried. The call does not say which of its fields is
/// the name — that would be per-server knowledge — so a couple of placements are tried and
/// the first that works wins. A placement that is wrong fails to match and changes
/// nothing, which is why searching this way is safe.
const MAX_PLACEMENTS: usize = 3;

impl TargetSearchRunner {
    /// Wraps `inner` so failed calls search its own reading before giving up.
    #[must_use]
    pub fn new(inner: Arc<dyn CapabilityRunner + Send + Sync>) -> Self {
        Self {
            inner,
            channels: Vec::new(),
        }
    }

    /// Wraps `inner`, with direct reach into the named servers (ADR 0054).
    #[must_use]
    pub fn with_channels(
        inner: Arc<dyn CapabilityRunner + Send + Sync>,
        channels: Vec<(String, Arc<dyn NativeChannel>)>,
    ) -> Self {
        Self { inner, channels }
    }

    /// The direct reach for whichever server owns `id`, if any.
    fn channel(&self, id: &str) -> Option<&Arc<dyn NativeChannel>> {
        let (server, _) = id.split_once('.')?;
        self.channels
            .iter()
            .find(|(name, _)| name == server)
            .map(|(_, channel)| channel)
    }

    /// This server's state as it is right now, via the tool the person nominated as its
    /// reader (ADR 0054). `None` when nobody nominated one — the honest silence, and the
    /// reason this whole mechanism needs no per-server code.
    fn reading(&self, id: &str) -> Option<String> {
        // Direct reach first: it reports everything the service knows, where the tool
        // surface reports only what that surface was configured to expose.
        if let Some(channel) = self.channel(id) {
            if let Ok(reading) = channel.reading() {
                return Some(reading);
            }
        }
        let verifier = self.inner.verifier(id)?;
        if verifier == id {
            return None; // a read that failed has nothing to look itself up in
        }
        self.inner.run(&verifier, "{}").ok()
    }

    /// The one candidate among the joint-best that the tool could actually operate.
    ///
    /// Live: "turn off the kitchen main light" tied three ways — `Kitchen Main Light`, its
    /// `LED` configuration entry, and its `Cloud connection` indicator. All three carry
    /// the device's name, and two of them are not controls at all. Reading the tie as
    /// genuine ambiguity meant refusing a request that had exactly one sensible answer.
    ///
    /// Only ever **narrows a tie**: it cannot overrule a clear winner, and if more than
    /// one operable thing remains the ambiguity was real and nothing is acted on.
    fn only_one_that_can_be_acted_on<'a>(
        &self,
        tool: &str,
        found: &'a [crate::target_search::Candidate],
    ) -> Option<&'a crate::target_search::Candidate> {
        let channel = self.channel(tool)?;
        let best = found.first()?;
        let tied: Vec<&crate::target_search::Candidate> =
            found.iter().filter(|c| c.matched == best.matched).collect();
        if tied.len() < 2 {
            return None; // not a tie; `only_real_match` already had its say
        }
        let known = channel.known().ok()?;
        let mut operable = tied.into_iter().filter(|candidate| {
            known
                .iter()
                .find(|(_, name)| name.eq_ignore_ascii_case(&candidate.value))
                .is_some_and(|(id, _)| channel.actionable(tool, id))
        });
        let only = operable.next()?;
        operable.next().is_none().then_some(only)
    }

    /// Acts on the matched name through the server's own interface, by id.
    ///
    /// `None` when this channel cannot express the tool, or does not know the name — the
    /// caller falls back to retrying the tool itself, so direct reach is an improvement
    /// and never a new way to fail.
    fn act_directly(
        &self,
        channel: &Arc<dyn NativeChannel>,
        tool: &str,
        name: &str,
    ) -> Option<Result<String, String>> {
        let known = channel.known().ok()?;
        let (entity, _) = known
            .iter()
            .find(|(_, known_name)| known_name.eq_ignore_ascii_case(name))?;
        match channel.act(tool, entity)? {
            // Say so. A person reading the trail should see that Endora went around the
            // tool surface, and exactly what it acted on (ADR 0053).
            Ok(out) => Some(Ok(format!(
                "(The first attempt failed. Endora looked up what actually exists and \
                 acted on '{name}' directly, as {entity}.)\n{out}"
            ))),
            // Falling back is the honest move: the tool retry may still work.
            Err(_) => None,
        }
    }

    /// The fields a real name could be placed in.
    ///
    /// **Every** scalar field, not only the ones already holding a fragment of the name.
    /// A call does not say which of its fields means "the name" — working that out would
    /// be the per-server knowledge this avoids — and the live failure was exactly a call
    /// that had the right words in the wrong field:
    ///
    /// ```text
    /// {area: "guest bedroom left", name: "lamp"}   -> INVALID_AREA
    /// ```
    ///
    /// `Guest Bedroom Left` is an entity, not an area. Restricting placements to
    /// fragment-holders left nowhere to put it: `area` already held the whole name, and
    /// `name` held "lamp", a word the house does not use. So it found the answer and had
    /// no way to try it.
    ///
    /// Fragment-holders first, most specific first, then everything else alphabetically —
    /// likeliest placement first, deterministic throughout. A field already holding the
    /// exact name is skipped: that call is the one that just failed.
    fn placements(input_json: &str, name: &str) -> Vec<String> {
        let lowered = name.to_lowercase();
        let mut fields: Vec<(bool, usize, String)> =
            crate::target_search::target_fields(input_json)
                .into_iter()
                .filter(|(_, value)| !value.eq_ignore_ascii_case(name))
                .map(|(field, value)| {
                    let fragment = crate::target_search::is_fragment_of(&value, &lowered);
                    (fragment, value.split_whitespace().count(), field)
                })
                .collect();
        fields.sort_by(|a, b| {
            b.0.cmp(&a.0)
                .then_with(|| b.1.cmp(&a.1))
                .then_with(|| a.2.cmp(&b.2))
        });
        fields.into_iter().map(|(_, _, field)| field).collect()
    }
}

impl CapabilityRunner for TargetSearchRunner {
    // Presence, states and the read-back scope are ANSWERED here, from this runner's own
    // channels, so they are written out below rather than forwarded.
    forwards_to_inner!(inner: decision, verifier);

    fn available(&self) -> Vec<crate::application::CapabilitySpec> {
        self.inner.available()
    }

    fn about_the_person(&self) -> Vec<String> {
        self.channels
            .iter()
            .filter_map(|(_, channel)| channel.about_the_person())
            .collect()
    }

    fn current_states(&self) -> Vec<(String, String)> {
        self.channels
            .iter()
            .flat_map(|(_, channel)| channel.states().unwrap_or_default())
            .collect()
    }

    fn read_back_input(&self, action_id: &str, action_input: &str) -> String {
        // Where there is direct reach, verification reads the WHOLE service, both before
        // and after (ADR 0053).
        //
        // Scoping the read to the action's arguments assumes those arguments name the
        // right thing — and this layer exists precisely because they often do not. Live:
        // a call aimed at "living room" was corrected to `light.kitchen_table` and acted
        // on it, while the read-back kept looking at the living room. Both readings were
        // identical, so a light the person watched come on was recorded as no change, and
        // the butler said it had failed.
        //
        // Reading everything cannot be aimed at the wrong thing. It can register an
        // unrelated change as this action's — which is the safe direction to be wrong,
        // since a false "nothing happened" is what causes false denials and nearly caused
        // the most useful tool in the house to be withdrawn.
        if self.channel(action_id).is_some() {
            return "{}".to_owned();
        }
        self.inner.read_back_input(action_id, action_input)
    }

    fn run(&self, id: &str, input_json: &str) -> Result<String, String> {
        // An exact identifier pins the target; anything else in the call can only widen
        // it (ADR 0054). Done BEFORE the call, because a call that widens and succeeds is
        // never seen by the recovery path below.
        let tightened = self
            .channel(id)
            .and_then(|c| c.tighten(input_json))
            .unwrap_or_else(|| input_json.to_owned());
        let input_json = tightened.as_str();
        if let Some(why) = self.channel(id).and_then(|c| c.refuse(id, input_json)) {
            return Err(why);
        }
        let first = self.inner.run(id, input_json);
        let Err(original) = first else {
            return first;
        };
        let Some(reading) = self.reading(id) else {
            return Err(original);
        };
        // A kind filter the service has never used as a category is not a category — it
        // is part of what the person named (ADR 0054). Only a service that can say so
        // changes anything here.
        let words = match self.channel(id).and_then(|c| c.categories().ok()) {
            Some(known) if !known.is_empty() => {
                crate::target_search::target_words_with_kinds(input_json, &known)
            }
            _ => crate::target_search::target_words(input_json),
        };
        let found = crate::target_search::candidates(&reading, &words);
        // Only an unambiguous match may be acted on. Everything else is shown — unless
        // the tie is only between a thing and its own diagnostics, which is not a real
        // ambiguity (ADR 0054).
        let settled = crate::target_search::only_real_match(&found)
            .or_else(|| self.only_one_that_can_be_acted_on(id, &found));
        let Some(best) = settled else {
            return Err(format!(
                "{original}{}",
                crate::target_search::shortlist(&found)
            ));
        };
        // Direct reach, where it exists: resolve the name to the service's own id and
        // act on exactly that (ADR 0054). An id cannot be mis-matched, so this is the end
        // of the guessing rather than a better guess.
        if let Some(channel) = self.channel(id) {
            if let Some(result) = self.act_directly(channel, id, &best.value) {
                return result;
            }
        }
        for field in Self::placements(input_json, &best.value)
            .into_iter()
            .take(MAX_PLACEMENTS)
        {
            let retry = crate::target_search::retarget(input_json, &field, &best.value);
            if let Ok(out) = self.inner.run(id, &retry) {
                // Say what was substituted, exactly as the alias recovery does. Nothing
                // about this is hidden from the model, the record, or the person.
                return Ok(format!(
                    "(The first attempt failed. Endora looked up what actually exists and \
                     retried against '{}'.)\n{out}",
                    best.value
                ));
            }
        }
        Err(format!(
            "{original}{}",
            crate::target_search::shortlist(&found)
        ))
    }
}

/// Hides the capabilities the person has **withdrawn** — turned off — from every
/// source, and refuses to run them (ADR 0054).
///
/// Turning a skill off has always worked for built-ins, because [`RegistryRunner`]
/// applies the stored flag itself. MCP tools had no equivalent: the flag could be set
/// and nothing happened, because the tools come from a shared connection built long
/// before the person's config is read. This applies it once, above every source, so
/// "off" means the same thing whatever the capability is and wherever it came from.
///
/// **Why hiding rather than blocking.** A blocked tool is still in the catalogue, and
/// the measured failure is the model *choosing the wrong tool* — it picks a brightness
/// setter to switch a light, is refused, and picks it again. Refusing more loudly does
/// not help; the tool has to stop being on the menu. Blocking is the answer for a tool
/// that works and is dangerous; withdrawal is the answer for one that does not work.
///
/// Reversible by construction: it reads a stored flag, so restoring a capability is one
/// click and nothing was lost.
pub struct WithdrawnRunner {
    inner: Arc<dyn CapabilityRunner + Send + Sync>,
    withdrawn: std::collections::HashSet<String>,
}

impl WithdrawnRunner {
    /// Wraps `inner`, hiding the given ids. Ids that no source offers are simply never
    /// matched, so a stale flag from a removed server is harmless.
    #[must_use]
    pub fn new(
        inner: Arc<dyn CapabilityRunner + Send + Sync>,
        withdrawn: std::collections::HashSet<String>,
    ) -> Self {
        Self { inner, withdrawn }
    }
}

impl CapabilityRunner for WithdrawnRunner {
    // Withdrawal is about which TOOLS are offered. What the services know about the person
    // and about the world is not a tool and is not withheld.
    forwards_to_inner!(inner: about_the_person, current_states, read_back_input);

    fn available(&self) -> Vec<crate::application::CapabilitySpec> {
        self.inner
            .available()
            .into_iter()
            .filter(|spec| !self.withdrawn.contains(&spec.id))
            .collect()
    }

    fn decision(&self, id: &str) -> Option<Decision> {
        if self.withdrawn.contains(id) {
            return None;
        }
        self.inner.decision(id)
    }

    fn verifier(&self, id: &str) -> Option<String> {
        // A withdrawn tool must not be nominated as anything's verifier either: it would
        // be asked to read state it can no longer be run to read.
        let verifier = self.inner.verifier(id)?;
        if self.withdrawn.contains(&verifier) {
            return None;
        }
        Some(verifier)
    }

    fn run(&self, id: &str, input_json: &str) -> Result<String, String> {
        // Refused here too, so a model naming it from earlier in the conversation — or a
        // direct call — cannot route around the narrowed catalogue.

        if self.withdrawn.contains(id) {
            return Err(format!("'{id}' is turned off"));
        }
        self.inner.run(id, input_json)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `CapabilityInfo` with a given band and reach; other fields are inert.
    fn info(reversibility: Reversibility, reaches_external: bool) -> CapabilityInfo {
        CapabilityInfo {
            id: "x",
            name: "X",
            description: "",
            category: "",
            reaches_external,
            reversibility,
            configured: true,
            needs: "",
            settings: &[],
            input_schema: None,
        }
    }

    #[test]
    fn one_stance_the_band_the_record_and_the_envelope_decide() {
        use crate::application::{AutonomyEnvelope, Stance};
        use Reversibility::{Irreversible, OutwardReversible, Reversible};
        let default_env = AutonomyEnvelope::default(); // external ok, consequential no
        let no_external = AutonomyEnvelope {
            auto_external: false,
            auto_consequential: false,
        };
        let widened = AutonomyEnvelope {
            auto_external: true,
            auto_consequential: true,
        };

        // The band's defaults, where the person has said nothing.
        assert_eq!(default_stance(Reversibility::Observe), Stance::Auto);
        assert_eq!(default_stance(Reversible), Stance::Auto);
        assert_eq!(default_stance(OutwardReversible), Stance::Ask);
        assert_eq!(default_stance(Irreversible), Stance::Off);

        // Auto acts — narrowed to confirm when an external action is kept in hand.
        assert_eq!(
            classify(&info(Reversible, false), &default_env, Stance::Auto, false),
            Decision::Act
        );
        assert_eq!(
            classify(&info(Reversible, true), &no_external, Stance::Auto, false),
            Decision::Confirm
        );

        // Ask confirms each use, whatever the envelope says...
        assert_eq!(
            classify(&info(OutwardReversible, true), &widened, Stance::Ask, false),
            Decision::Confirm
        );
        // ...until the record proves the tool AND the envelope allows consequential
        // actions — then it acts. Graduation (ADR 0062).
        assert_eq!(
            classify(&info(OutwardReversible, true), &widened, Stance::Ask, true),
            Decision::Act
        );
        // Narrow the envelope and every graduate asks again.
        assert_eq!(
            classify(
                &info(OutwardReversible, true),
                &default_env,
                Stance::Ask,
                true
            ),
            Decision::Confirm
        );

        // Off blocks, whoever set it and whyever — proof and widening change nothing.
        assert_eq!(
            classify(&info(Irreversible, true), &widened, Stance::Off, true),
            Decision::Block
        );

        // `may_run_autonomously` is exactly "the verdict is Act".
        assert!(may_run_autonomously(
            &info(Reversible, false),
            &default_env,
            Stance::Auto,
            false
        ));
        assert!(!may_run_autonomously(
            &info(OutwardReversible, true),
            &widened,
            Stance::Ask,
            false
        ));
    }

    #[test]
    fn run_refuses_an_irreversible_skill_deny_by_default() {
        // A skill whose effect can't be undone must be refused by the execution
        // path itself, not just excluded from autonomous runs (ADR 0051).
        struct IrreversibleSkill;
        impl Capability for IrreversibleSkill {
            fn info(&self) -> CapabilityInfo {
                CapabilityInfo {
                    id: "wire_transfer",
                    name: "Wire transfer",
                    description: "",
                    category: "",
                    reaches_external: true,
                    reversibility: Reversibility::Irreversible,
                    configured: true,
                    needs: "",
                    settings: &[],
                    input_schema: None,
                }
            }
            fn invoke(
                &self,
                _input: &Value,
                _settings: &CapabilitySettings,
            ) -> Result<Value, CapabilityError> {
                panic!("an irreversible skill must never be invoked");
            }
        }
        let runner = RegistryRunner::new(Arc::new(vec![
            Arc::new(IrreversibleSkill) as Arc<dyn Capability>
        ]));
        let err = runner.run("wire_transfer", "{}").unwrap_err();
        assert!(err.contains("can't be undone"), "unexpected error: {err}");
    }

    #[test]
    fn run_allows_an_opened_irreversible_skill_but_never_autonomously() {
        // Once the person opens a capability's irreversible band, the execution path
        // runs it (reached only via an explicit confirmation) — but it is still
        // never cleared to act on its own (ADR 0051).
        struct BookingSkill;
        impl Capability for BookingSkill {
            fn info(&self) -> CapabilityInfo {
                CapabilityInfo {
                    id: "booking",
                    name: "Booking",
                    description: "",
                    category: "",
                    reaches_external: true,
                    reversibility: Reversibility::Irreversible,
                    configured: true,
                    needs: "",
                    settings: &[],
                    input_schema: None,
                }
            }
            fn invoke(
                &self,
                _input: &Value,
                _settings: &CapabilitySettings,
            ) -> Result<Value, CapabilityError> {
                Ok(json!({ "booked": true }))
            }
        }
        let caps: Arc<Vec<Arc<dyn Capability>>> =
            Arc::new(vec![Arc::new(BookingSkill) as Arc<dyn Capability>]);

        // Moved to `ask` for this capability, fully-widened envelope, nothing proven.
        let runner = RegistryRunner::with_config(
            caps,
            vec![("booking".to_owned(), crate::application::Stance::Ask)],
            std::collections::HashSet::new(),
            crate::application::AutonomyEnvelope {
                auto_external: true,
                auto_consequential: true,
            },
            std::collections::HashMap::new(),
        );

        // It runs now (the person confirmed) — no longer blocked.
        assert_eq!(runner.run("booking", "{}").unwrap(), r#"{"booked":true}"#);
        // But it is never autonomous: it must be confirmed every time.
        let spec = runner
            .available()
            .into_iter()
            .find(|s| s.id == "booking")
            .unwrap();
        assert!(
            !spec.autonomous,
            "an unproven irreversible skill at `ask` must still confirm"
        );
    }

    #[test]
    fn decision_reports_the_full_verdict_including_block() {
        // `decision` exposes the real Act/Confirm/Block verdict (for the audit
        // trail), unlike the coarse `autonomous` bool which collapses the last two.
        struct BookingSkill;
        impl Capability for BookingSkill {
            fn info(&self) -> CapabilityInfo {
                CapabilityInfo {
                    id: "booking",
                    name: "Booking",
                    description: "",
                    category: "",
                    reaches_external: true,
                    reversibility: Reversibility::Irreversible,
                    configured: true,
                    needs: "",
                    settings: &[],
                    input_schema: None,
                }
            }
            fn invoke(
                &self,
                _input: &Value,
                _settings: &CapabilitySettings,
            ) -> Result<Value, CapabilityError> {
                Ok(json!({}))
            }
        }
        let caps: Arc<Vec<Arc<dyn Capability>>> =
            Arc::new(vec![Arc::new(BookingSkill) as Arc<dyn Capability>]);

        // Closed: the un-undoable is Blocked (the bool would only say "not autonomous").
        let closed = RegistryRunner::new(caps.clone());
        assert_eq!(closed.decision("booking"), Some(Decision::Block));
        // At `ask`, unproven: Confirm — never Act.
        let opened = RegistryRunner::with_config(
            caps,
            vec![("booking".to_owned(), crate::application::Stance::Ask)],
            std::collections::HashSet::new(),
            crate::application::AutonomyEnvelope::default(),
            std::collections::HashMap::new(),
        );
        assert_eq!(opened.decision("booking"), Some(Decision::Confirm));
        // Unknown skill: no verdict.
        assert_eq!(closed.decision("nope"), None);
    }

    #[test]
    fn registry_has_modules_with_stable_ids() {
        let caps = default_capabilities();
        assert!(caps.len() >= 6);
        let ids: Vec<_> = caps.iter().map(|c| c.info().id).collect();
        assert!(ids.contains(&"weather"));
        assert!(ids.contains(&"web_fetch"));
        assert!(ids.contains(&"flights"));
    }

    #[test]
    fn a_scaffold_reports_unavailable_with_a_reason() {
        let err = FlightSearchCapability
            .invoke(&json!({}), &CapabilitySettings::new())
            .unwrap_err();
        assert!(matches!(err, CapabilityError::Unavailable(_)));
    }

    #[test]
    fn observed_time_formats_the_open_meteo_timestamp() {
        assert_eq!(
            observed_time("2026-07-23T07:00").as_deref(),
            Some("7:00 AM")
        );
        assert_eq!(
            observed_time("2026-07-23T00:15").as_deref(),
            Some("12:15 AM")
        );
        assert_eq!(
            observed_time("2026-07-23T13:30").as_deref(),
            Some("1:30 PM")
        );
        assert_eq!(
            observed_time("2026-07-23T12:00").as_deref(),
            Some("12:00 PM")
        );
        // Unrecognized shapes fall back to no tag (rather than a wrong one).
        assert_eq!(observed_time("garbage"), None);
        assert_eq!(observed_time("2026-07-23T99:00"), None);
    }

    #[test]
    fn geocode_candidates_simplify_city_state() {
        assert_eq!(
            geocode_candidates("New York NY"),
            vec!["New York NY", "New York"]
        );
        assert_eq!(
            geocode_candidates("New York, NY"),
            vec!["New York, NY", "New York"]
        );
        assert_eq!(geocode_candidates("Boston"), vec!["Boston"]);
        assert_eq!(geocode_candidates("San Francisco"), vec!["San Francisco"]);
    }

    #[test]
    fn zip_detector_ignores_non_zip_input_without_a_network_call() {
        // Non-5-digit or non-numeric input must fall through (Ok(None)), so a place
        // name never triggers the ZIP lookup.
        assert!(resolve_us_zip("New York").unwrap().is_none());
        assert!(resolve_us_zip("2827").unwrap().is_none());
        assert!(resolve_us_zip("abcde").unwrap().is_none());
        assert!(resolve_us_zip("Boston, MA").unwrap().is_none());
    }

    #[test]
    fn outbound_tripwire_flags_secrets_but_not_ordinary_text() {
        // Ordinary queries and URLs are NOT flagged (no false positives).
        assert_eq!(scan_outbound_secret("what's the weather in New York"), None);
        assert_eq!(
            scan_outbound_secret("https://example.com/articles/2026/summer-festival"),
            None
        );
        assert_eq!(scan_outbound_secret("{\"location\":\"10001\"}"), None);
        // Known credential shapes ARE flagged.
        assert_eq!(
            scan_outbound_secret("{\"q\":\"my key is sk-abcdefabcdefabcdefabcdef\"}"),
            Some("an API key")
        );
        assert_eq!(
            scan_outbound_secret("AKIAIOSFODNN7EXAMPLE and more"),
            Some("an AWS access key")
        );
        assert_eq!(
            scan_outbound_secret("ghp_1234567890abcdefghijklmnopqrstuv"),
            Some("a GitHub token")
        );
        assert!(scan_outbound_secret("-----BEGIN RSA PRIVATE KEY-----").is_some());
        // A secret embedded in a URL query string is caught too.
        assert_eq!(
            scan_outbound_secret("https://x.com/?token=sk-abcdef1234567890abcdef"),
            Some("an API key")
        );
        assert_eq!(
            scan_outbound_secret("token eyJhbGciOi.eyJzdWIiOi.SflKxwRJSM"),
            Some("a token (JWT)")
        );
    }

    #[test]
    fn query_minimization_redacts_emails_but_not_urls_or_plain_text() {
        // A standalone email in a query is redacted.
        assert_eq!(
            redact_emails_in_text("email john.doe@example.com about the trip"),
            "email [redacted-email] about the trip"
        );
        // Trailing punctuation is handled; the rest of the word survives.
        assert_eq!(
            redact_emails_in_text("(contact a@b.com)"),
            "(contact [redacted-email])"
        );
        // A URL is one word and not an email — left intact.
        assert_eq!(
            redact_emails_in_text("https://site.com/path?x=1"),
            "https://site.com/path?x=1"
        );
        // Ordinary text is untouched.
        assert_eq!(
            redact_emails_in_text("weather in New York tomorrow"),
            "weather in New York tomorrow"
        );
        assert!(!looks_like_email("https://a.com/x@y"));
        assert!(looks_like_email("a@b.co"));
    }

    #[test]
    fn query_minimization_walks_json_values() {
        let mut v = json!({ "query": "reach me at me@x.org", "url": "https://ok.com/" });
        redact_pii_in_value(&mut v);
        assert_eq!(v["query"], json!("reach me at [redacted-email]"));
        assert_eq!(v["url"], json!("https://ok.com/")); // url untouched
    }

    #[test]
    fn egress_guard_blocks_internal_ip_literals() {
        // Loopback, RFC1918, link-local (incl. cloud metadata), and IPv6 loopback.
        for url in [
            "http://127.0.0.1/x",
            "http://192.168.1.10:8787/data",
            "http://10.0.0.5/",
            "http://169.254.169.254/latest/meta-data",
            "https://[::1]/",
            "http://[fd00::1]/",
        ] {
            assert!(
                matches!(guard_egress(url), Err(CapabilityError::BadInput(_))),
                "should have blocked {url}"
            );
        }
        // A non-http scheme is refused too.
        assert!(guard_egress("ftp://8.8.8.8/").is_err());
    }

    #[test]
    fn egress_guard_allows_public_ip_literals() {
        assert!(guard_egress("https://8.8.8.8/").is_ok());
        assert!(guard_egress("http://1.1.1.1:8080/x").is_ok());
    }

    #[test]
    fn host_and_port_parses_forms() {
        assert_eq!(
            host_and_port("https://example.com/path"),
            Some(("example.com".to_owned(), 443))
        );
        assert_eq!(
            host_and_port("http://example.com:8080/x?y=1"),
            Some(("example.com".to_owned(), 8080))
        );
        assert_eq!(
            host_and_port("http://user:pass@10.0.0.1/x"),
            Some(("10.0.0.1".to_owned(), 80))
        );
        assert_eq!(
            host_and_port("https://[::1]:9000/"),
            Some(("::1".to_owned(), 9000))
        );
    }

    #[test]
    fn redirect_resolution_handles_absolute_and_relative() {
        assert_eq!(
            resolve_redirect("https://a.com/x", "https://b.com/y"),
            "https://b.com/y"
        );
        assert_eq!(
            resolve_redirect("https://a.com/x/y", "/z"),
            "https://a.com/z"
        );
    }

    #[test]
    fn web_fetch_rejects_non_http() {
        let err = WebFetchCapability
            .invoke(
                &json!({ "url": "file:///etc/passwd" }),
                &CapabilitySettings::new(),
            )
            .unwrap_err();
        assert!(matches!(err, CapabilityError::BadInput(_)));
    }

    #[test]
    fn rss_items_are_extracted_with_titles_links_and_publisher() {
        let xml = "<rss><channel>\
            <item><title>Storms hit New York &amp; the region</title><link>https://ex.com/a</link>\
              <source url=\"https://www.wcnc.com\">WCNC</source></item>\
            <item><title><![CDATA[City council votes tonight]]></title></item>\
            <item><title>Third &#39;big&#39; story</title></item>\
            </channel></rss>";
        let items = extract_rss_items(xml, 6);
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].0, "Storms hit New York & the region");
        assert_eq!(items[0].1, "https://ex.com/a"); // the source link is kept
        assert_eq!(items[0].2, "WCNC"); // publisher name from <source>
        assert_eq!(items[1].0, "City council votes tonight");
        assert_eq!(items[1].1, ""); // no link in the feed ⇒ empty, not fabricated
        assert_eq!(items[1].2, ""); // no <source> ⇒ empty, not fabricated
        assert_eq!(items[2].0, "Third 'big' story");
    }

    #[test]
    fn news_without_a_place_or_query_is_bad_input() {
        let err = LocalNewsCapability
            .invoke(&json!({}), &CapabilitySettings::new())
            .unwrap_err();
        assert!(matches!(err, CapabilityError::BadInput(_)));
    }

    #[test]
    fn knowledge_summarize_reads_the_extract_or_says_nothing_found() {
        let hit = json!({ "query": "Ada Lovelace", "found": true, "title": "Ada Lovelace",
            "extract": "Ada Lovelace was an English mathematician." });
        assert_eq!(
            KnowledgeCapability.summarize(&hit),
            "Ada Lovelace: Ada Lovelace was an English mathematician."
        );
        let miss = json!({ "query": "asdfqwer", "found": false, "extract": "" });
        assert!(
            KnowledgeCapability
                .summarize(&miss)
                .contains("couldn't find")
        );
    }

    #[test]
    fn nothing_claims_to_search_the_web_any_more() {
        // The DuckDuckGo skill is gone, and this is here so it does not quietly return.
        //
        // It called the Instant Answer API, which is not a search engine: it answers when a
        // query names a well-known entity and returns **nothing** for real questions. Live,
        // an ordinary question about local events came back empty, and so did one naming a
        // stadium. It had been offered to the model for months and called zero times
        // across thirty recorded outcomes — and had it been called, it would have said nothing.
        //
        // Two skills claiming to search, one of them useless, gives a model no way to choose
        // and a person no way to tell. Better to offer nothing than something that cannot work.
        let searching: Vec<&str> = default_capabilities()
            .iter()
            .map(|c| c.info().id)
            .filter(|id| *id == "web_search")
            .collect();
        assert!(
            searching.is_empty(),
            "something is claiming to search again: {searching:?}"
        );
    }

    #[test]
    fn home_assistant_needs_config_and_summarizes_state() {
        // Without url/token it's unavailable (never reaches the home).
        assert!(
            HomeAssistantCapability
                .invoke(&json!({}), &CapabilitySettings::new())
                .is_err()
        );
        // Summary reads entities as name: state.
        let out = json!({ "domain": "light", "count": 2, "entities": [
            { "entity": "light.kitchen", "name": "Kitchen", "state": "on" },
            { "entity": "light.desk", "name": "Desk", "state": "off" },
        ] });
        let s = HomeAssistantCapability.summarize(&out);
        assert!(s.contains("Kitchen: on"));
        assert!(s.contains("Desk: off"));
    }

    #[test]
    fn news_summarize_gives_a_readable_numbered_list() {
        let out = json!({
            "query": "New York news",
            "count": 2,
            "headlines": ["Council meets tonight", "Road closures downtown"],
            "note": "",
        });
        let text = LocalNewsCapability.summarize(&out);
        assert!(text.contains("New York news"));
        assert!(text.contains("1. Council meets tonight"));
        assert!(text.contains("2. Road closures downtown"));
        // Empty results read as a plain "none found", never as raw JSON.
        let empty = json!({ "query": "Nowhere news", "count": 0, "headlines": [] });
        assert!(
            LocalNewsCapability
                .summarize(&empty)
                .contains("No recent news")
        );
    }

    #[test]
    fn strip_html_drops_tags_and_scripts() {
        let t = strip_html("<html><script>bad()</script><p>Hello <b>world</b></p></html>");
        assert_eq!(t, "Hello world");
    }

    #[test]
    fn image_review_declares_a_model_setting_and_needs_it() {
        let cap = ImageReviewCapability {
            ollama_base: "http://localhost:11434".to_owned(),
        };
        // It declares a required "model" setting...
        assert_eq!(cap.info().settings.len(), 1);
        assert_eq!(cap.info().settings[0].key, "model");
        // ...and without it, running fails (never reaches the vision call).
        assert!(
            cap.invoke(
                &json!({ "image_url": "http://x/y.png" }),
                &CapabilitySettings::new()
            )
            .is_err()
        );
    }

    #[test]
    fn settings_complete_requires_every_declared_setting() {
        let cap = ImageReviewCapability {
            ollama_base: "http://localhost:11434".to_owned(),
        };
        let info = cap.info();
        assert!(!settings_complete(&info, &CapabilitySettings::new()));
        let mut s = CapabilitySettings::new();
        s.insert("model".to_owned(), "moondream".to_owned());
        assert!(settings_complete(&info, &s));
        // A keyless skill is always complete.
        assert!(settings_complete(
            &WeatherCapability.info(),
            &CapabilitySettings::new()
        ));
    }

    // ---- MCP host: adapter + composite -------------------------------------

    use crate::application::CapabilitySpec;

    /// A stand-in MCP server. `call` echoes `tool(input)` so a test can prove which
    /// tool was reached; an unhealthy server fails `list_tools`. No interior
    /// mutability, so it stays `Send + Sync` for `Box<dyn McpClient>`.
    struct FakeTransport {
        tools: Vec<McpToolInfo>,
        healthy: bool,
    }

    impl McpClient for FakeTransport {
        fn list_tools(&self) -> Result<Vec<McpToolInfo>, String> {
            if self.healthy {
                Ok(self.tools.clone())
            } else {
                Err("server down".to_owned())
            }
        }
        fn call(&self, tool: &str, input_json: &str) -> Result<String, String> {
            Ok(format!("{tool}({input_json})"))
        }
    }

    /// A stand-in MCP server that also offers **resources** — the standard half of the
    /// protocol Endora did not speak (ADR 0058).
    struct FakeWithResources {
        resources: Vec<McpResource>,
        unreadable: bool,
    }

    impl McpClient for FakeWithResources {
        fn list_tools(&self) -> Result<Vec<McpToolInfo>, String> {
            Ok(vec![tool("do_something")])
        }
        fn call(&self, tool: &str, input: &str) -> Result<String, String> {
            Ok(format!("{tool}({input})"))
        }
        fn list_resources(&self) -> Result<Vec<McpResource>, String> {
            Ok(self.resources.clone())
        }
        fn read_resource(&self, uri: &str) -> Result<String, String> {
            if self.unreadable {
                return Err("gone".to_owned());
            }
            Ok(format!("state of {uri}"))
        }
    }

    fn resource(uri: &str) -> McpResource {
        McpResource {
            uri: uri.to_owned(),
            name: uri.to_owned(),
            description: String::new(),
        }
    }

    #[test]
    fn an_mcp_servers_resources_become_states_the_watch_loop_can_see() {
        // The payoff of speaking the other half of MCP: a third-party server can now feed the
        // watch loop, the transition log and notions with no Rust in this repository at all.
        let runner = McpRunner::connect_with_readers(vec![(
            "house".to_owned(),
            Box::new(FakeWithResources {
                resources: vec![resource("house://light.kitchen"), resource("house://door")],
                unreadable: false,
            }) as Box<dyn McpClient>,
            String::new(),
        )]);

        let mut states = runner.current_states();
        states.sort();
        assert_eq!(
            states,
            vec![
                (
                    "house::house://door".to_owned(),
                    "state of house://door".to_owned()
                ),
                (
                    "house::house://light.kitchen".to_owned(),
                    "state of house://light.kitchen".to_owned()
                ),
            ],
            "states must say which server they came from"
        );
    }

    #[test]
    fn two_servers_publishing_the_same_name_do_not_collide() {
        // A flat key would have one silently overwrite the other, and both would still look
        // present — the kind of loss nothing downstream could detect.
        let runner = McpRunner::connect_with_readers(vec![
            (
                "house".to_owned(),
                Box::new(FakeWithResources {
                    resources: vec![resource("status")],
                    unreadable: false,
                }) as Box<dyn McpClient>,
                String::new(),
            ),
            (
                "network".to_owned(),
                Box::new(FakeWithResources {
                    resources: vec![resource("status")],
                    unreadable: false,
                }) as Box<dyn McpClient>,
                String::new(),
            ),
        ]);
        let mut keys: Vec<String> = runner
            .current_states()
            .into_iter()
            .map(|(key, _)| key)
            .collect();
        keys.sort();
        assert_eq!(keys, vec!["house::status", "network::status"]);
    }

    #[test]
    fn a_tools_only_server_reports_no_states_rather_than_failing() {
        // The ordinary case. Most MCP servers have no resources, and that must never look
        // like trouble — the watch loop treats absence as something to raise.
        let runner = McpRunner::connect_with_readers(vec![(
            "plain".to_owned(),
            Box::new(FakeTransport {
                tools: vec![tool("search")],
                healthy: true,
            }) as Box<dyn McpClient>,
            String::new(),
        )]);
        assert!(runner.current_states().is_empty());
    }

    #[test]
    fn a_resource_that_will_not_read_is_skipped_not_fatal() {
        // One unreadable resource must not cost the watch loop every other reading it had.
        let runner = McpRunner::connect_with_readers(vec![(
            "house".to_owned(),
            Box::new(FakeWithResources {
                resources: vec![resource("house://gone")],
                unreadable: true,
            }) as Box<dyn McpClient>,
            String::new(),
        )]);
        assert!(runner.current_states().is_empty());
    }

    fn tool(name: &str) -> McpToolInfo {
        McpToolInfo {
            name: name.to_owned(),
            description: format!("does {name}"),
            input_schema: None,
        }
    }

    /// A tool that advertises the arguments it takes, so schema-against-schema logic
    /// has something real to match.
    fn schema_tool(name: &str, fields: &[&str]) -> McpToolInfo {
        let props: serde_json::Map<String, Value> = fields
            .iter()
            .map(|f| ((*f).to_owned(), serde_json::json!({ "type": "string" })))
            .collect();
        McpToolInfo {
            name: name.to_owned(),
            description: format!("does {name}"),
            input_schema: Some(serde_json::json!({ "type": "object", "properties": props })),
        }
    }

    #[test]
    fn mcp_runner_namespaces_tools_is_deny_by_default_and_routes() {
        let transport = FakeTransport {
            tools: vec![tool("create_event"), tool("list_events")],
            healthy: true,
        };
        let runner = McpRunner::connect(vec![("calendar".to_owned(), Box::new(transport))]);

        // Tools appear namespaced, configured, and never autonomous.
        let ids: Vec<String> = runner.available().into_iter().map(|s| s.id).collect();
        assert_eq!(ids, vec!["calendar.create_event", "calendar.list_events"]);
        assert!(
            runner
                .available()
                .iter()
                .all(|s| s.configured && !s.autonomous)
        );

        // Deny-by-default: a hosted tool blocks; an unknown id is None.
        assert_eq!(
            runner.decision("calendar.create_event"),
            Some(Decision::Block)
        );
        assert_eq!(runner.decision("calendar.nope"), None);
        assert_eq!(runner.decision("weather"), None);

        // `run` routes faithfully to the transport (its contract: only called once
        // policy has cleared the tool).
        assert_eq!(
            runner.run("calendar.create_event", "{\"x\":1}").unwrap(),
            "create_event({\"x\":1})"
        );
        assert!(runner.run("calendar.nope", "{}").is_err());
    }

    #[test]
    fn coerce_wraps_a_scalar_where_the_schema_wants_an_array() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "domain": { "type": "array" },
                "name": { "type": "string" }
            }
        });
        // The classic HA slip: domain sent as a string, not an array.
        let out = coerce_args_to_schema(r#"{"domain":"light","name":"kitchen"}"#, Some(&schema));
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["domain"], serde_json::json!(["light"]));
        // A non-array field is left alone; an already-array value is untouched.
        assert_eq!(v["name"], "kitchen");
        let already = coerce_args_to_schema(r#"{"domain":["light"]}"#, Some(&schema));
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&already).unwrap()["domain"],
            serde_json::json!(["light"])
        );
        // No schema, or unparseable input → passed through unchanged.
        assert_eq!(coerce_args_to_schema("{bad", Some(&schema)), "{bad");
        assert_eq!(coerce_args_to_schema(r#"{"x":1}"#, None), r#"{"x":1}"#);
    }

    /// The hint names what a tool **needs**, and counts the rest.
    ///
    /// It used to list every parameter a server declared. A search tool with nine of them
    /// read to a small model as nine things to collect, and it asked the person for them —
    /// "the count of results you'd like to see, your preferred country, what level of
    /// freshness you require". That is somebody else's API surface, put to the person as a
    /// question. Optional means the server has a default.
    #[test]
    fn compact_input_hint_names_what_is_needed_and_counts_the_rest() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "name": { "type": "string" },
                "area": { "type": "string" }
            },
            "required": ["name"]
        });
        let hint = compact_input_hint(&schema).unwrap();
        assert!(hint.contains("name (string)"), "got: {hint}");
        assert!(
            !hint.contains("area"),
            "an optional field was named: {hint}"
        );
        assert!(hint.contains("1 more"), "got: {hint}");

        // Everything optional: say so rather than listing a menu to be collected.
        let all_optional = serde_json::json!({
            "type": "object",
            "properties": { "count": { "type": "integer" }, "country": { "type": "string" } }
        });
        let hint = compact_input_hint(&all_optional).unwrap();
        assert!(hint.contains("no arguments"), "got: {hint}");
        assert!(!hint.contains("country"), "got: {hint}");
        // A no-argument tool needs no hint.
        assert!(
            compact_input_hint(&serde_json::json!({ "type": "object", "properties": {} }))
                .is_none()
        );
        assert!(compact_input_hint(&serde_json::json!({})).is_none());
    }

    #[test]
    fn mcp_available_folds_the_input_schema_into_the_description() {
        let mut turn_on = tool("HassTurnOn");
        turn_on.description = "Turns on a device".to_owned();
        turn_on.input_schema = Some(serde_json::json!({
            "type": "object",
            "properties": { "name": { "type": "string" } },
            "required": ["name"]
        }));
        let transport = FakeTransport {
            tools: vec![turn_on],
            healthy: true,
        };
        let runner = McpRunner::connect(vec![("home".to_owned(), Box::new(transport))]);
        let spec = runner
            .available()
            .into_iter()
            .find(|s| s.id == "home.HassTurnOn")
            .unwrap();
        // The model sees both what the tool does AND how to call it.
        assert!(spec.description.contains("Turns on a device"));
        // What it NEEDS, not every parameter it will accept — a menu of optional fields
        // reads to a small model as things to go and collect from the person.
        assert!(
            spec.description.contains("needs: name (string)"),
            "description should carry the input hint, got: {}",
            spec.description
        );
    }

    #[test]
    fn mcp_runner_skips_an_unhealthy_server() {
        let ok = FakeTransport {
            tools: vec![tool("read_file")],
            healthy: true,
        };
        let down = FakeTransport {
            tools: vec![tool("send_mail")],
            healthy: false,
        };
        let runner = McpRunner::connect(vec![
            ("files".to_owned(), Box::new(ok)),
            ("mail".to_owned(), Box::new(down)),
        ]);
        // Only the healthy server contributes tools; the down one drops out silently.
        let ids: Vec<String> = runner.available().into_iter().map(|s| s.id).collect();
        assert_eq!(ids, vec!["files.read_file"]);
    }

    /// A minimal built-in-like source: an autonomous `weather` skill.
    struct FakeBuiltin;
    impl CapabilityRunner for FakeBuiltin {
        fn available(&self) -> Vec<CapabilitySpec> {
            vec![CapabilitySpec {
                id: "weather".to_owned(),
                description: "the weather".to_owned(),
                configured: true,
                autonomous: true,
                input_schema: None,
                reversibility: endora_kernel::Reversibility::Observe,
            }]
        }
        fn run(&self, id: &str, _input: &str) -> Result<String, String> {
            if id == "weather" {
                Ok("sunny".to_owned())
            } else {
                Err(format!("no such skill '{id}'"))
            }
        }
    }

    #[test]
    fn composite_runner_merges_sources_and_routes_by_owner() {
        let mcp = McpRunner::connect(vec![(
            "calendar".to_owned(),
            Box::new(FakeTransport {
                tools: vec![tool("create_event")],
                healthy: true,
            }) as Box<dyn McpClient>,
        )]);
        let composite = CompositeRunner::new(vec![Arc::new(FakeBuiltin), Arc::new(mcp)]);

        // Both sources' catalogs are merged.
        let ids: Vec<String> = composite.available().into_iter().map(|s| s.id).collect();
        assert_eq!(ids, vec!["weather", "calendar.create_event"]);

        // Each source keeps its own policy verdict.
        assert_eq!(composite.decision("weather"), Some(Decision::Act));
        assert_eq!(
            composite.decision("calendar.create_event"),
            Some(Decision::Block)
        );
        assert_eq!(composite.decision("missing"), None);

        // `run` dispatches to whichever source owns the id.
        assert_eq!(composite.run("weather", "{}").unwrap(), "sunny");
        assert_eq!(
            composite.run("calendar.create_event", "{}").unwrap(),
            "create_event({})"
        );
        assert!(composite.run("missing", "{}").is_err());
    }

    #[test]
    fn a_confirmed_alias_recovers_a_call_that_failed_on_the_name() {
        // The live case, after hours of it: the entity is `Kitchen Table`, the model kept
        // sending `name: "table"`, and Home Assistant kept answering
        // no_match_reason=NAME. With the alias in its prompt it STILL sent "table" —
        // grounding is advice, and this model takes advice about a third of the time.
        struct OnlyKnowsTheRealName;
        impl CapabilityRunner for OnlyKnowsTheRealName {
            fn available(&self) -> Vec<crate::application::CapabilitySpec> {
                vec![crate::application::CapabilitySpec {
                    id: "home-assistant.HassTurnOn".to_owned(),
                    description: String::new(),
                    configured: true,
                    autonomous: true,
                    input_schema: None,
                    reversibility: Reversibility::Irreversible,
                }]
            }
            fn run(&self, _id: &str, input: &str) -> Result<String, String> {
                if input.contains("Kitchen Table") {
                    return Ok("action_done".to_owned());
                }
                Err("no_match_reason=NAME".to_owned())
            }
        }

        let runner = AliasRunner::new(
            Arc::new(OnlyKnowsTheRealName),
            vec![(
                "home-assistant".to_owned(),
                "table".to_owned(),
                "Kitchen Table".to_owned(),
            )],
        );

        let out = runner
            .run("home-assistant.HassTurnOn", r#"{"name":"table"}"#)
            .expect("the alias recovers it");
        assert!(out.contains("action_done"));
        // And it says what it did — the substitution is visible to the model, the outcome
        // record and the person. Recovering silently would hide the model's mistake.
        assert!(
            out.contains("retried") && out.contains("Kitchen Table"),
            "the retry was silent: {out}"
        );
    }

    #[test]
    fn an_alias_never_touches_a_call_that_worked() {
        // Recovery, never pre-emption: it fires only after a failure, so it can never
        // redirect a call that was about to hit the right thing.
        struct AlwaysWorks;
        impl CapabilityRunner for AlwaysWorks {
            fn available(&self) -> Vec<crate::application::CapabilitySpec> {
                Vec::new()
            }
            fn run(&self, _id: &str, input: &str) -> Result<String, String> {
                Ok(format!("ran with {input}"))
            }
        }
        let runner = AliasRunner::new(
            Arc::new(AlwaysWorks),
            vec![(
                "home-assistant".to_owned(),
                "table".to_owned(),
                "Kitchen Table".to_owned(),
            )],
        );
        let out = runner
            .run("home-assistant.HassTurnOn", r#"{"name":"table"}"#)
            .unwrap();
        assert!(
            out.contains(r#""table""#),
            "a working call was rewritten: {out}"
        );
        assert!(!out.contains("Kitchen Table"));
    }

    #[test]
    fn an_alias_matches_a_whole_value_not_a_fragment() {
        // "table" must not rewrite "comfortable". Saying what one thing is called is not
        // licence to edit every value containing that word.
        struct Fails;
        impl CapabilityRunner for Fails {
            fn available(&self) -> Vec<crate::application::CapabilitySpec> {
                Vec::new()
            }
            fn run(&self, _id: &str, _input: &str) -> Result<String, String> {
                Err("nope".to_owned())
            }
        }
        let runner = AliasRunner::new(
            Arc::new(Fails),
            vec![(
                "home-assistant".to_owned(),
                "table".to_owned(),
                "Kitchen Table".to_owned(),
            )],
        );
        // No whole-value match, so no retry input exists and the original error stands.
        assert_eq!(
            runner.run(
                "home-assistant.HassTurnOn",
                r#"{"name":"comfortable chair"}"#
            ),
            Err("nope".to_owned())
        );
    }

    #[test]
    fn the_nominated_reader_is_an_observation_not_a_receipt() {
        // Observed live: GetLiveContext's own result came back stamped
        //   "[unverified] This is what the tool reported about its own work."
        // A read has no work of its own, and that text tells the model not to trust the
        // very reading ADR 0053 uses as confirmation.
        let mcp = McpRunner::connect_with_readers(vec![(
            "home-assistant".to_owned(),
            Box::new(FakeTransport {
                tools: vec![tool("HassTurnOff"), tool("GetLiveContext")],
                healthy: true,
            }) as Box<dyn McpClient>,
            "GetLiveContext".to_owned(),
        )]);

        let specs = mcp.available();
        let reader = specs
            .iter()
            .find(|s| s.id == "home-assistant.GetLiveContext")
            .expect("the reader is offered");
        assert_eq!(reader.reversibility, Reversibility::Observe);
        assert!(reader.autonomous, "a read may run on its own");
        assert_eq!(
            mcp.decision("home-assistant.GetLiveContext"),
            Some(Decision::Act)
        );

        // Everything else on the server stays deny-by-default, and is verified through
        // the nominated reader.
        let action = specs
            .iter()
            .find(|s| s.id == "home-assistant.HassTurnOff")
            .expect("the action is offered");
        assert_eq!(action.reversibility, Reversibility::Irreversible);
        assert!(!action.autonomous);
        assert_eq!(
            mcp.verifier("home-assistant.HassTurnOff").as_deref(),
            Some("home-assistant.GetLiveContext")
        );
        // A reader never verifies itself.
        assert_eq!(mcp.verifier("home-assistant.GetLiveContext"), None);
    }

    #[test]
    fn arguments_the_schema_forbids_are_dropped_not_sent() {
        // Live failure: device_class:["light"] — not one of that field's permitted
        // values — failed the WHOLE call with a validation error, so a light nobody
        // could name stayed on. The schema says which values are allowed; use it.
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "name": { "type": "string" },
                "floor": { "type": "string" },
                "device_class": {
                    "type": "array",
                    "items": { "enum": ["outlet", "switch", "tv"] }
                }
            }
        });
        let out = coerce_args_to_schema(
            r#"{"name":"main","floor":"","device_class":["light"]}"#,
            Some(&schema),
        );
        let parsed: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed["name"], "main");
        assert!(
            parsed.get("device_class").is_none(),
            "an invalid enum value was sent: {out}"
        );
        assert!(
            parsed.get("floor").is_none(),
            "an empty filter was sent: {out}"
        );
    }

    #[test]
    fn cleaning_a_call_never_turns_one_target_into_all_of_them() {
        // Observed live, and caused by the empty-field hygiene I added:
        //   HassTurnOn{area:null, name:null, domain:["light"]}
        // cleaned to {domain:["light"]} — and Home Assistant turned on every light in
        // the house. The model plainly meant to name something and sent nulls; the
        // honest reading is "it failed to say what", not "it meant all of them".
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "area": { "type": "string" },
                "name": { "type": "string" },
                "domain": { "type": "array", "items": { "type": "string" } }
            }
        });
        let out = coerce_args_to_schema(
            r#"{"area":null,"name":null,"domain":["light"]}"#,
            Some(&schema),
        );
        let parsed: Value = serde_json::from_str(&out).unwrap();
        assert!(
            parsed.get("area").is_some() || parsed.get("name").is_some(),
            "the empties were cleaned away, leaving a house-wide action: {out}"
        );
    }

    #[test]
    fn a_call_with_one_real_target_is_still_cleaned() {
        // The guard must not undo the fix it sits next to: an empty `floor` alongside a
        // real name still gets dropped, because the call still aims at something.
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "name": { "type": "string" },
                "floor": { "type": "string" },
                "domain": { "type": "array", "items": { "type": "string" } }
            }
        });
        let out = coerce_args_to_schema(
            r#"{"name":"Kitchen Table","floor":"","domain":["light"]}"#,
            Some(&schema),
        );
        let parsed: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed["name"], "Kitchen Table");
        assert!(parsed.get("floor").is_none(), "empty floor survived: {out}");
    }

    #[test]
    fn a_tool_that_takes_no_target_is_left_alone() {
        // Something like "cancel all timers" supplies no targeting field at all. It never
        // aimed at one thing, so there is nothing for the guard to protect.
        let schema = serde_json::json!({ "type": "object", "properties": {} });
        let out = coerce_args_to_schema("{}", Some(&schema));
        assert_eq!(out, "{}");
    }

    #[test]
    fn a_kind_filter_that_merely_repeats_another_is_dropped() {
        // Live: HassTurnOff{area:"kitchen", domain:["switch"], device_class:["switch"]}
        // — the same word twice. `Kitchen Main Light` is domain `switch` with no
        // matching device_class, so the pair excluded everything and Home Assistant
        // reported the AREA unmatched even though it exists.
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "area": { "type": "string" },
                "domain": { "type": "array", "items": { "type": "string" } },
                "device_class": { "type": "array", "items": { "type": "string" } }
            }
        });
        let out = coerce_args_to_schema(
            r#"{"area":"kitchen","domain":["switch"],"device_class":["switch"]}"#,
            Some(&schema),
        );
        let parsed: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed["area"], "kitchen", "the target is untouched");
        // Exactly one of the duplicated pair survives, so the call constrains the same
        // amount it always did — dropping it cannot widen the blast radius.
        let kept = ["domain", "device_class"]
            .iter()
            .filter(|k| parsed.get(**k).is_some())
            .count();
        assert_eq!(kept, 1, "one filter should remain: {out}");
    }

    #[test]
    fn genuinely_different_kind_filters_both_survive() {
        // domain:["light"] with device_class:["outlet"] is a real narrowing, not a
        // duplication. Dropping either would change what the call can touch.
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "domain": { "type": "array", "items": { "type": "string" } },
                "device_class": { "type": "array", "items": { "type": "string" } }
            }
        });
        let out = coerce_args_to_schema(
            r#"{"domain":["light"],"device_class":["outlet"]}"#,
            Some(&schema),
        );
        let parsed: Value = serde_json::from_str(&out).unwrap();
        assert!(parsed.get("domain").is_some(), "{out}");
        assert!(parsed.get("device_class").is_some(), "{out}");
    }

    #[test]
    fn a_permitted_value_survives_the_hygiene() {
        // The check must not eat valid arguments — that would break every working call.
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "device_class": {
                    "type": "array",
                    "items": { "enum": ["outlet", "switch"] }
                }
            }
        });
        let out = coerce_args_to_schema(r#"{"device_class":"switch"}"#, Some(&schema));
        let parsed: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(
            parsed["device_class"],
            serde_json::json!(["switch"]),
            "a valid value was coerced to an array and kept: {out}"
        );
    }

    #[test]
    fn the_read_back_is_scoped_to_what_the_action_targeted() {
        // The live failure: "turn the kitchen main off" read state back with no
        // arguments, got every device in the house, and the butler answered about the
        // GARAGE — the first thing in the dump that happened to be on.
        //
        // Home Assistant's GetLiveContext accepts name/domain/area filters, and the turn
        // was passing none of them.
        let mcp = McpRunner::connect_with_readers(vec![(
            "home-assistant".to_owned(),
            Box::new(FakeTransport {
                tools: vec![
                    schema_tool("HassTurnOff", &["name", "area", "domain"]),
                    schema_tool("GetLiveContext", &["name", "area", "domain"]),
                ],
                healthy: true,
            }) as Box<dyn McpClient>,
            "GetLiveContext".to_owned(),
        )]);

        let scoped = mcp.read_back_input(
            "home-assistant.HassTurnOff",
            r#"{"area":"kitchen","name":"main"}"#,
        );
        let parsed: Value = serde_json::from_str(&scoped).unwrap();
        assert_eq!(parsed["area"], "kitchen");
        assert_eq!(parsed["name"], "main");
    }

    #[test]
    fn a_kind_filter_never_narrows_the_read_back() {
        // The live case: "kitchen main" is a SWITCH, and the model sent
        // domain: ["light"]. Inheriting that would hide the one entity that explains
        // the failure — after an action that did not land, what is actually there is
        // the whole point of looking.
        let mcp = McpRunner::connect_with_readers(vec![(
            "home-assistant".to_owned(),
            Box::new(FakeTransport {
                tools: vec![
                    schema_tool("HassTurnOff", &["name", "area", "domain"]),
                    schema_tool("GetLiveContext", &["name", "area", "domain"]),
                ],
                healthy: true,
            }) as Box<dyn McpClient>,
            "GetLiveContext".to_owned(),
        )]);

        let scoped = mcp.read_back_input(
            "home-assistant.HassTurnOff",
            r#"{"area":"kitchen","name":"main","domain":["light"]}"#,
        );
        let parsed: Value = serde_json::from_str(&scoped).unwrap();
        assert_eq!(parsed["area"], "kitchen", "location still narrows");
        assert!(
            parsed.get("domain").is_none(),
            "the kind filter leaked into the reading: {scoped}"
        );
    }

    #[test]
    fn a_reader_only_gets_arguments_it_understands() {
        // Schema against schema: an argument the reader does not accept must not be
        // forwarded, or the read-back fails and the action goes unverified.
        let mcp = McpRunner::connect_with_readers(vec![(
            "home-assistant".to_owned(),
            Box::new(FakeTransport {
                tools: vec![
                    schema_tool("HassTurnOff", &["name", "brightness"]),
                    schema_tool("GetLiveContext", &["name"]),
                ],
                healthy: true,
            }) as Box<dyn McpClient>,
            "GetLiveContext".to_owned(),
        )]);

        let scoped = mcp.read_back_input(
            "home-assistant.HassTurnOff",
            r#"{"name":"kitchen","brightness":40}"#,
        );
        let parsed: Value = serde_json::from_str(&scoped).unwrap();
        assert_eq!(parsed["name"], "kitchen");
        assert!(
            parsed.get("brightness").is_none(),
            "an argument the reader can't take was forwarded: {scoped}"
        );
    }

    #[test]
    fn an_untargeted_action_still_reads_the_whole_state() {
        // No overlap, no scoping — the whole reading, which is the old behaviour and
        // the right fallback.
        let mcp = McpRunner::connect_with_readers(vec![(
            "home-assistant".to_owned(),
            Box::new(FakeTransport {
                tools: vec![
                    schema_tool("HassTurnOff", &["name"]),
                    schema_tool("GetLiveContext", &["name"]),
                ],
                healthy: true,
            }) as Box<dyn McpClient>,
            "GetLiveContext".to_owned(),
        )]);
        assert_eq!(
            mcp.read_back_input("home-assistant.HassTurnOff", "{}"),
            "{}"
        );
    }

    #[test]
    fn any_server_gets_read_back_once_someone_nominates_its_reader() {
        // ADR 0054's whole point. Nothing here is Home Assistant, and nothing in the
        // runner knows what this server is — a calendar gets verification on exactly
        // the same terms, because the mapping is data rather than a name in the source.
        let mcp = McpRunner::connect_with_readers(vec![(
            "calendar".to_owned(),
            Box::new(FakeTransport {
                tools: vec![tool("create_event"), tool("list_events")],
                healthy: true,
            }) as Box<dyn McpClient>,
            "list_events".to_owned(),
        )]);

        assert_eq!(
            mcp.verifier("calendar.create_event").as_deref(),
            Some("calendar.list_events"),
            "an action is verified through the nominated reader"
        );
        let reader = mcp
            .available()
            .into_iter()
            .find(|s| s.id == "calendar.list_events")
            .expect("offered");
        assert_eq!(reader.reversibility, Reversibility::Observe);
        assert!(reader.autonomous);
    }

    #[test]
    fn a_server_with_no_nomination_gets_no_reader_and_no_read_back() {
        // The honest default, unchanged: Endora does not guess which tool reads, and a
        // server's own say-so is not evidence (ADR 0051/0038). Note this holds even for
        // a tool literally named GetLiveContext — the old hardcode is gone.
        let mcp = McpRunner::connect(vec![(
            "home-assistant".to_owned(),
            Box::new(FakeTransport {
                tools: vec![tool("HassTurnOff"), tool("GetLiveContext")],
                healthy: true,
            }) as Box<dyn McpClient>,
        )]);
        assert_eq!(mcp.verifier("home-assistant.HassTurnOff"), None);
        for spec in mcp.available() {
            assert_eq!(spec.reversibility, Reversibility::Irreversible);
            assert!(!spec.autonomous);
        }
    }

    #[test]
    fn opener_runner_lifts_block_to_confirm_only_for_opened_tools() {
        let mcp = Arc::new(McpRunner::connect(vec![(
            "fs".to_owned(),
            Box::new(FakeTransport {
                tools: vec![tool("read_file"), tool("write_file")],
                healthy: true,
            }) as Box<dyn McpClient>,
        )]));
        // The person has opened only fs.write_file, but has NOT allowed acting on its
        // own (auto_consequential = false).
        let stances: std::collections::HashMap<String, Stance> =
            [("fs.write_file".to_owned(), Stance::Ask)].into();
        let overlay = OpenerRunner::new(mcp, stances, std::collections::HashSet::new(), false);

        // Deny-by-default holds for the un-opened tool; the opened one becomes
        // confirm-each-use (never autonomous — its spec stays non-autonomous).
        assert_eq!(overlay.decision("fs.read_file"), Some(Decision::Block));
        assert_eq!(overlay.decision("fs.write_file"), Some(Decision::Confirm));
        assert!(overlay.available().iter().all(|s| !s.autonomous));

        // Only the opened tool may run; the blocked one is refused even on a direct
        // call. The opened tool routes through to the transport.
        assert!(overlay.run("fs.read_file", "{}").is_err());
        assert_eq!(
            overlay.run("fs.write_file", "{\"p\":1}").unwrap(),
            "write_file({\"p\":1})"
        );
    }

    #[test]
    fn unattended_runner_keeps_an_opened_actuator_from_running_with_nobody_there() {
        // The exact configuration the nightly loop's safety claim has to survive: the
        // person opened a Home Assistant actuator AND widened the envelope, so in a
        // chat turn `OpenerRunner` clears it to act on its own. Unattended, it must not.
        let mcp = Arc::new(McpRunner::connect(vec![(
            "home".to_owned(),
            Box::new(FakeTransport {
                tools: vec![tool("HassTurnOff")],
                healthy: true,
            }) as Box<dyn McpClient>,
        )]));
        let stances: std::collections::HashMap<String, Stance> =
            [("home.HassTurnOff".to_owned(), Stance::Ask)].into();
        let proven: std::collections::HashSet<String> = ["home.HassTurnOff".to_owned()].into();
        let attended = Arc::new(OpenerRunner::new(mcp, stances, proven, true));
        // Precondition: attended, it really is cleared to act.
        assert_eq!(attended.decision("home.HassTurnOff"), Some(Decision::Act));

        let unattended = ReversibleOnlyRunner::new(attended);
        // Unattended, the same tool falls back to needing a person.
        assert_eq!(
            unattended.decision("home.HassTurnOff"),
            Some(Decision::Confirm)
        );
        assert!(unattended.available().iter().all(|s| !s.autonomous));
        // And it is refused at the run layer too, not merely un-offered.
        assert!(unattended.run("home.HassTurnOff", "{}").is_err());
    }

    #[test]
    fn unattended_runner_leaves_reversible_skills_alone() {
        // The nightly loop still has to be able to research: an `Observe`-band skill
        // keeps acting on its own, which is the whole point of the loop.
        let unattended = ReversibleOnlyRunner::new(Arc::new(FakeBuiltin));
        assert_eq!(unattended.decision("weather"), Some(Decision::Act));
        assert!(unattended.available().iter().all(|s| s.autonomous));
        assert_eq!(unattended.run("weather", "{}").unwrap(), "sunny");
    }

    #[test]
    fn unattended_runner_treats_an_unknown_band_as_an_actuator() {
        // Deny-by-default, same rule ADR 0053 applies to verification: a capability
        // whose band we cannot see is assumed to change something.
        let unattended = ReversibleOnlyRunner::new(Arc::new(FakeBuiltin));
        assert!(unattended.run("mystery", "{}").is_err());
    }

    #[test]
    fn opener_runner_lets_opened_tools_act_when_autonomy_is_widened() {
        let mcp = Arc::new(McpRunner::connect(vec![(
            "home".to_owned(),
            Box::new(FakeTransport {
                tools: vec![tool("HassTurnOn"), tool("HassTurnOff")],
                healthy: true,
            }) as Box<dyn McpClient>,
        )]));
        let stances: std::collections::HashMap<String, Stance> =
            [("home.HassTurnOn".to_owned(), Stance::Ask)].into();
        let proven: std::collections::HashSet<String> = ["home.HassTurnOn".to_owned()].into();
        // Both gates open (ADR 0062): the record has proven this tool AND the person
        // allowed acting on consequential things on its own.
        let overlay = OpenerRunner::new(mcp, stances, proven, true);

        // The opened tool may now run in the loop (Act + autonomous); the un-opened
        // one is still blocked, and never autonomous.
        assert_eq!(overlay.decision("home.HassTurnOn"), Some(Decision::Act));
        assert_eq!(overlay.decision("home.HassTurnOff"), Some(Decision::Block));
        let specs = overlay.available();
        assert!(
            specs
                .iter()
                .find(|s| s.id == "home.HassTurnOn")
                .unwrap()
                .autonomous
        );
        assert!(
            !specs
                .iter()
                .find(|s| s.id == "home.HassTurnOff")
                .unwrap()
                .autonomous
        );
    }

    /// A runner offering two tools, one of which the person turned off.
    fn two_tools() -> Arc<dyn CapabilityRunner + Send + Sync> {
        struct Two;
        impl CapabilityRunner for Two {
            fn available(&self) -> Vec<crate::application::CapabilitySpec> {
                ["home.HassTurnOn", "home.HassLightSet"]
                    .into_iter()
                    .map(|id| crate::application::CapabilitySpec {
                        id: id.to_owned(),
                        description: String::new(),
                        configured: true,
                        autonomous: false,
                        reversibility: Reversibility::Reversible,
                        input_schema: None,
                    })
                    .collect()
            }
            fn decision(&self, _id: &str) -> Option<Decision> {
                Some(Decision::Confirm)
            }
            fn run(&self, id: &str, _input: &str) -> Result<String, String> {
                Ok(format!("ran {id}"))
            }
            fn verifier(&self, _id: &str) -> Option<String> {
                Some("home.HassLightSet".to_owned())
            }
            fn read_back_input(&self, _id: &str, _input: &str) -> String {
                "{}".to_owned()
            }
        }
        Arc::new(Two)
    }

    fn withdrawing(id: &str) -> WithdrawnRunner {
        WithdrawnRunner::new(two_tools(), [id.to_owned()].into_iter().collect())
    }

    #[test]
    fn a_withdrawn_capability_is_not_on_the_menu_at_all() {
        // The measured failure is the model CHOOSING the wrong tool, so refusing it more
        // loudly does not help — it has to stop being offered.
        let runner = withdrawing("home.HassLightSet");
        let ids: Vec<String> = runner.available().into_iter().map(|s| s.id).collect();
        assert_eq!(ids, vec!["home.HassTurnOn".to_owned()]);
    }

    #[test]
    fn a_withdrawn_capability_cannot_be_run_by_name() {
        // The model may name a tool it saw earlier in the conversation, and a direct
        // call must not route around the narrowed catalogue either.
        let err = withdrawing("home.HassLightSet")
            .run("home.HassLightSet", "{}")
            .expect_err("a withdrawn tool ran");
        assert!(err.contains("turned off"), "{err}");
        assert!(
            withdrawing("home.HassLightSet")
                .decision("home.HassLightSet")
                .is_none(),
            "a withdrawn tool still had a policy decision"
        );
    }

    #[test]
    fn withdrawing_a_reader_leaves_actions_unverified_rather_than_broken() {
        // Read-back must not name a tool that can no longer be run: the turn would ask
        // for a reading it cannot get. Unverified is the honest fallback (ADR 0053).
        let runner = withdrawing("home.HassLightSet");
        assert_eq!(runner.verifier("home.HassTurnOn"), None);
    }

    #[test]
    fn everything_else_is_untouched() {
        let runner = withdrawing("home.HassLightSet");
        assert_eq!(
            runner.run("home.HassTurnOn", "{}").unwrap(),
            "ran home.HassTurnOn"
        );
        assert_eq!(runner.decision("home.HassTurnOn"), Some(Decision::Confirm));
    }

    /// A server that behaves like the live one: it matches a name only when it is exactly
    /// right, and it has a reader that reports what exists.
    struct House {
        calls: std::sync::Mutex<Vec<String>>,
    }

    impl CapabilityRunner for House {
        fn available(&self) -> Vec<crate::application::CapabilitySpec> {
            Vec::new()
        }
        fn decision(&self, _id: &str) -> Option<Decision> {
            Some(Decision::Act)
        }
        fn verifier(&self, id: &str) -> Option<String> {
            (id != "home.GetLiveContext").then(|| "home.GetLiveContext".to_owned())
        }
        fn read_back_input(&self, _id: &str, _input: &str) -> String {
            "{}".to_owned()
        }
        fn run(&self, id: &str, input: &str) -> Result<String, String> {
            if id == "home.GetLiveContext" {
                return Ok(
                    "{\"result\": \"- names: Kitchen Main Light\\n  domain: light\\n\
                           - names: Kitchen Table\\n  domain: light\\n\
                           - names: Guest Bedroom Left\\n  domain: light\\n\
                           - names: Guest Bedroom\\n  domain: light\\n\
                           - names: Garage Main\\n  domain: light\\n\"}"
                        .to_owned(),
                );
            }
            self.calls.lock().unwrap().push(input.to_owned());
            let v: Value = serde_json::from_str(input).unwrap();
            // Matches by name, exactly — like the real thing. An entity name in the AREA
            // field is refused, which is the live INVALID_AREA failure.
            let named = v.get("name").and_then(Value::as_str).unwrap_or_default();
            if ["Kitchen Table", "Guest Bedroom Left"].contains(&named) {
                return Ok(format!("turned on {named}"));
            }
            Err("MatchFailedError no_match_reason=NAME".to_owned())
        }
    }

    fn house() -> (Arc<House>, TargetSearchRunner) {
        let inner = Arc::new(House {
            calls: std::sync::Mutex::new(Vec::new()),
        });
        let runner =
            TargetSearchRunner::new(Arc::clone(&inner) as Arc<dyn CapabilityRunner + Send + Sync>);
        (inner, runner)
    }

    #[test]
    fn it_finds_the_name_the_model_spent_fourteen_attempts_failing_to_guess() {
        // The live failure, end to end: the model asks for "table" in the kitchen, the
        // server refuses, and the name it needed was in the reading all along.
        let (inner, runner) = house();
        let out = runner
            .run(
                "home.HassTurnOn",
                r#"{"name":"table","area":"kitchen","domain":["light"]}"#,
            )
            .expect("the search did not recover the call");
        assert!(out.contains("turned on Kitchen Table"), "{out}");
        assert!(
            out.contains("Kitchen Table"),
            "the substitution is disclosed: {out}"
        );
        let calls = inner.calls.lock().unwrap();
        assert!(
            calls.iter().any(|c| c.contains("Kitchen Table")),
            "never retried with the real name: {calls:?}"
        );
    }

    #[test]
    fn a_working_call_is_never_touched() {
        // Recovery only. It must not be able to hijack a call that was already right.
        let (inner, runner) = house();
        let out = runner
            .run("home.HassTurnOn", r#"{"name":"Kitchen Table"}"#)
            .unwrap();
        assert_eq!(out, "turned on Kitchen Table", "rewrote a working call");
        assert_eq!(
            inner.calls.lock().unwrap().len(),
            1,
            "retried unnecessarily"
        );
    }

    #[test]
    fn an_ambiguous_search_shows_the_names_and_acts_on_none_of_them() {
        // "kitchen" resembles two lights. Picking one would be a coin flip that actuates
        // something, so it hands over the shortlist and stops.
        let (inner, runner) = house();
        let err = runner
            .run("home.HassTurnOn", r#"{"area":"kitchen"}"#)
            .expect_err("acted on an ambiguous match");
        assert!(err.contains("Kitchen Table"), "{err}");
        assert!(err.contains("Kitchen Main Light"), "{err}");
        assert!(err.contains("EXACTLY"), "does not say to copy it: {err}");
        assert_eq!(
            inner.calls.lock().unwrap().len(),
            1,
            "retried on a guess: {:?}",
            inner.calls.lock().unwrap()
        );
    }

    #[test]
    fn nothing_resembling_it_leaves_the_original_failure_alone() {
        let (_, runner) = house();
        let err = runner
            .run("home.HassTurnOn", r#"{"name":"greenhouse"}"#)
            .expect_err("invented a match");
        assert!(err.contains("MatchFailedError"), "{err}");
        assert!(
            !err.contains("[candidates]"),
            "offered nothing as something: {err}"
        );
    }

    #[test]
    fn a_server_with_no_nominated_reader_searches_nothing() {
        // No reader means no reading, which means no candidates — the same honest silence
        // ADR 0054 chose, and the reason this needs no per-server code.
        struct Blind;
        impl CapabilityRunner for Blind {
            fn available(&self) -> Vec<crate::application::CapabilitySpec> {
                Vec::new()
            }
            fn decision(&self, _id: &str) -> Option<Decision> {
                Some(Decision::Act)
            }
            fn verifier(&self, _id: &str) -> Option<String> {
                None
            }
            fn read_back_input(&self, _id: &str, _input: &str) -> String {
                "{}".to_owned()
            }
            fn run(&self, _id: &str, _input: &str) -> Result<String, String> {
                Err("nope".to_owned())
            }
        }
        let runner = TargetSearchRunner::new(Arc::new(Blind));
        assert_eq!(
            runner.run("x.Y", r#"{"name":"table"}"#).unwrap_err(),
            "nope"
        );
    }

    #[test]
    fn the_retry_is_bounded_however_many_fields_the_call_carried() {
        let (inner, runner) = house();
        let _ = runner.run(
            "home.HassTurnOn",
            r#"{"name":"table","area":"kitchen","floor":"kitchen table","zone":"table"}"#,
        );
        assert!(
            inner.calls.lock().unwrap().len() <= 1 + MAX_PLACEMENTS,
            "unbounded retrying: {:?}",
            inner.calls.lock().unwrap()
        );
    }

    #[test]
    fn it_moves_a_real_name_into_a_field_that_can_hold_it() {
        // Live, and the search found the answer then had nowhere to put it:
        //   {area: "guest bedroom left", name: "lamp"} -> INVALID_AREA
        // `Guest Bedroom Left` is an entity, not an area. The field already held the
        // whole name and the name field held "lamp", so restricting placements to
        // fragment-holders left no placement at all.
        let (inner, runner) = house();
        let out = runner
            .run(
                "home.HassTurnOn",
                r#"{"area":"guest bedroom left","name":"lamp","domain":["light"]}"#,
            )
            .expect("found the name and could not use it");
        assert!(out.contains("turned on Guest Bedroom Left"), "{out}");
        let calls = inner.calls.lock().unwrap();
        assert!(
            calls
                .iter()
                .any(|c| c.contains(r#""name":"Guest Bedroom Left""#)),
            "never tried the name in a field that could hold it: {calls:?}"
        );
    }

    /// A service with its own interface: things have ids, and acting by id cannot miss.
    struct Direct {
        acted: std::sync::Mutex<Vec<String>>,
    }

    impl NativeChannel for Direct {
        fn known(&self) -> Result<Vec<(String, String)>, String> {
            Ok(vec![
                ("light.kitchen_table".to_owned(), "Kitchen Table".to_owned()),
                (
                    "switch.kitchen_main".to_owned(),
                    "Kitchen Main Light".to_owned(),
                ),
                (
                    "light.hidden_from_assist".to_owned(),
                    "Pantry Strip".to_owned(),
                ),
            ])
        }
        fn reading(&self) -> Result<String, String> {
            // Ids are deliberately absent: `light.kitchen_table` shares its words with
            // `Kitchen Table` and would compete with it as a candidate.
            Ok("names: Kitchen Table\n\
                names: Kitchen Main Light\n\
                names: Kitchen Main Light LED\n\
                names: Kitchen Main Light Cloud connection\n\
                names: Pantry Strip"
                .to_owned())
        }
        fn act(&self, tool: &str, id: &str) -> Option<Result<String, String>> {
            if !tool.ends_with("HassTurnOn") {
                return None;
            }
            self.acted.lock().unwrap().push(id.to_owned());
            Some(Ok(format!("called turn_on on {id}")))
        }

        fn about_the_person(&self) -> Option<String> {
            Some("john is not home".to_owned())
        }

        fn states(&self) -> Result<Vec<(String, String)>, String> {
            Ok(vec![
                ("Kitchen Table".to_owned(), "off".to_owned()),
                ("Kitchen Main Light".to_owned(), "on".to_owned()),
            ])
        }

        fn actionable(&self, _tool: &str, id: &str) -> bool {
            id.starts_with("light.") || id.starts_with("switch.")
        }
    }

    fn with_direct() -> (Arc<Direct>, TargetSearchRunner) {
        let direct = Arc::new(Direct {
            acted: std::sync::Mutex::new(Vec::new()),
        });
        // Wrapping the server itself, not the plain search runner — nesting two of them
        // would let the inner one recover first, and the outer would never see a failure.
        let server = Arc::new(House {
            calls: std::sync::Mutex::new(Vec::new()),
        });
        let runner = TargetSearchRunner::with_channels(
            server as Arc<dyn CapabilityRunner + Send + Sync>,
            vec![(
                "home".to_owned(),
                Arc::clone(&direct) as Arc<dyn NativeChannel>,
            )],
        );
        (direct, runner)
    }

    #[test]
    fn direct_reach_acts_by_id_rather_than_retrying_a_name() {
        // The end of the guessing. The model asked for "table"; the service's own
        // interface has `light.kitchen_table`, which cannot be mis-matched.
        let (direct, runner) = with_direct();
        let out = runner
            .run("home.HassTurnOn", r#"{"name":"table","area":"kitchen"}"#)
            .expect("direct reach did not act");
        assert!(out.contains("light.kitchen_table"), "{out}");
        assert_eq!(
            *direct.acted.lock().unwrap(),
            vec!["light.kitchen_table".to_owned()]
        );
    }

    #[test]
    fn it_can_find_what_the_tool_surface_never_exposed() {
        // The reason direct reach is worth having beyond exactness: `Pantry Strip` is not
        // in the tool surface's reading at all, so nothing Endora did before could find
        // it however hard it searched.
        let (direct, runner) = with_direct();
        let out = runner
            .run("home.HassTurnOn", r#"{"name":"pantry strip"}"#)
            .expect("could not reach something hidden from the tool surface");
        assert!(out.contains("light.hidden_from_assist"), "{out}");
        assert_eq!(direct.acted.lock().unwrap().len(), 1);
    }

    #[test]
    fn a_tool_the_channel_cannot_express_falls_back_to_the_retry() {
        // Direct reach is an improvement, never a new way to fail: the fake channel only
        // expresses turning on, so turning off goes back through the tool.
        let (direct, runner) = with_direct();
        let out = runner.run("home.HassTurnOff", r#"{"name":"table","area":"kitchen"}"#);
        assert!(direct.acted.lock().unwrap().is_empty(), "acted anyway");
        assert!(out.is_err() || out.unwrap().contains("Kitchen Table"));
    }

    #[test]
    fn direct_reach_still_refuses_an_ambiguous_match() {
        // Exactness does not lower the bar for acting. "kitchen" resembles two things,
        // and having ids available does not make choosing between them safe.
        let (direct, runner) = with_direct();
        let err = runner
            .run("home.HassTurnOn", r#"{"area":"kitchen"}"#)
            .expect_err("acted on an ambiguous match");
        assert!(err.contains("Kitchen Table"), "{err}");
        assert!(direct.acted.lock().unwrap().is_empty(), "acted on a guess");
    }

    #[test]
    fn a_confirmed_name_drops_the_models_other_guesses() {
        // Live, with the alias already in place and still failing:
        //   {name:"table light", area:"Living Room", floor:"1"} -> INVALID_FLOOR
        // A kitchen light placed on an invented floor in the wrong room. The server
        // rejected the call on the floor and never looked at the name, so substituting
        // it changed nothing.
        struct Picky;
        impl CapabilityRunner for Picky {
            fn available(&self) -> Vec<crate::application::CapabilitySpec> {
                Vec::new()
            }
            fn decision(&self, _id: &str) -> Option<Decision> {
                Some(Decision::Act)
            }
            fn verifier(&self, _id: &str) -> Option<String> {
                None
            }
            fn read_back_input(&self, _id: &str, _input: &str) -> String {
                "{}".to_owned()
            }
            fn run(&self, _id: &str, input: &str) -> Result<String, String> {
                let v: Value = serde_json::from_str(input).unwrap();
                if v.get("floor").is_some() {
                    return Err("INVALID_FLOOR".to_owned());
                }
                if v.get("name").and_then(Value::as_str) == Some("Kitchen Table") {
                    return Ok("turned off Kitchen Table".to_owned());
                }
                Err("no match".to_owned())
            }
        }
        let runner = AliasRunner::new(
            Arc::new(Picky),
            vec![(
                "home".to_owned(),
                "table light".to_owned(),
                "Kitchen Table".to_owned(),
            )],
        );
        let out = runner
            .run(
                "home.HassTurnOff",
                r#"{"name":"table light","area":"Living Room","floor":"1","domain":["light"]}"#,
            )
            .expect("the confirmed name never got a chance");
        assert!(out.contains("turned off Kitchen Table"), "{out}");
    }

    #[test]
    fn a_confirmed_name_still_never_widens_the_call() {
        // Kind filters restrict which sorts of thing count rather than claiming which one
        // it is. Dropping them would widen the call, which is how one action became every
        // light in the house.
        let runner = AliasRunner::new(
            Arc::new(House {
                calls: std::sync::Mutex::new(Vec::new()),
            }),
            vec![(
                "home".to_owned(),
                "table".to_owned(),
                "Kitchen Table".to_owned(),
            )],
        );
        let applied = runner
            .apply(
                "home.HassTurnOn",
                r#"{"name":"table","area":"kitchen","domain":["light"]}"#,
            )
            .expect("the alias did not apply");
        let v: Value = serde_json::from_str(&applied).unwrap();
        assert_eq!(v["name"], "Kitchen Table");
        assert_eq!(
            v["domain"],
            serde_json::json!(["light"]),
            "widened: {applied}"
        );
        assert!(
            v.get("area").is_none(),
            "kept a guess about the same target: {applied}"
        );
    }

    #[test]
    fn a_thing_and_its_own_diagnostics_are_not_a_real_ambiguity() {
        // Live: "turn off the kitchen main light" tied three ways — the light, its LED
        // configuration entry, and its cloud-connection indicator. All three carry the
        // device's name; two of them are not controls at all. Reading that as ambiguity
        // meant refusing a request with exactly one sensible answer.
        let (direct, runner) = with_direct();
        let out = runner
            .run("home.HassTurnOn", r#"{"name":"kitchen main light"}"#)
            .expect("refused a request with one operable answer");
        assert!(out.contains("switch.kitchen_main"), "{out}");
        assert_eq!(
            *direct.acted.lock().unwrap(),
            vec!["switch.kitchen_main".to_owned()],
            "acted on a diagnostic"
        );
    }

    #[test]
    fn a_tie_between_two_real_things_still_acts_on_neither() {
        // Narrowing a tie must not become a way to break one. Two operable candidates is
        // the ambiguity this whole path exists to refuse.
        let (direct, runner) = with_direct();
        let err = runner
            .run("home.HassTurnOn", r#"{"area":"kitchen"}"#)
            .expect_err("acted on a genuine ambiguity");
        assert!(err.contains("Kitchen Table"), "{err}");
        assert!(direct.acted.lock().unwrap().is_empty(), "acted on a guess");
    }

    #[test]
    fn direct_reach_verifies_against_the_whole_service() {
        // Live: a call aimed at "living room" was corrected to `light.kitchen_table` and
        // acted on it, while the read-back kept looking at the living room. Both readings
        // matched, so a light the person watched come on was recorded as no change.
        let (_, runner) = with_direct();
        assert_eq!(
            runner.read_back_input("home.HassTurnOn", r#"{"area":"living room"}"#),
            "{}",
            "verification still aimed where the model pointed"
        );
    }

    #[test]
    fn without_direct_reach_the_scoped_read_back_is_untouched() {
        let (_, runner) = house();
        assert_eq!(
            runner.read_back_input("home.HassTurnOn", r#"{"area":"kitchen"}"#),
            "{}",
            "the inner runner decides when there is no channel"
        );
    }

    #[test]
    fn what_a_service_knows_about_the_person_reaches_the_turn() {
        // The house was already reporting `person.john -> not_home` in a reading Endora
        // fetches for other reasons, and nothing ever looked at it. A butler that does not
        // know whether anyone is in is guessing every time it decides whether to speak.
        let (_, runner) = with_direct();
        assert_eq!(
            runner.about_the_person(),
            vec!["john is not home".to_owned()]
        );
    }

    #[test]
    fn a_service_with_nothing_to_say_about_them_says_nothing() {
        let (_, plain) = house();
        assert!(plain.about_the_person().is_empty());
    }

    #[test]
    fn an_enum_nested_in_any_of_is_still_a_list_of_permitted_values() {
        // The shape that got through. A generated schema usually says "a string from this
        // list, or null" as an anyOf, and looking only at the top two levels missed it —
        // so `device_class: ["light"]` was sent, and Home Assistant answered
        // `'light' is not one of ['awning', 'blind', ...]`, failing the whole call.
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "device_class": {
                    "anyOf": [
                        { "type": "array", "items": { "enum": ["door", "garage"] } },
                        { "type": "null" }
                    ]
                }
            }
        });
        let out = coerce_args_to_schema(
            r#"{"device_class":["light"],"name":"front"}"#,
            Some(&schema),
        );
        let v: Value = serde_json::from_str(&out).unwrap();
        assert!(
            v.get("device_class").is_none(),
            "sent a value the schema rejects: {out}"
        );
        assert_eq!(v["name"], "front", "dropped something valid: {out}");
    }

    #[test]
    fn a_permitted_value_nested_the_same_way_survives() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "device_class": {
                    "anyOf": [
                        { "type": "array", "items": { "enum": ["door", "garage"] } },
                        { "type": "null" }
                    ]
                }
            }
        });
        let out = coerce_args_to_schema(r#"{"device_class":["garage"]}"#, Some(&schema));
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["device_class"], serde_json::json!(["garage"]));
    }

    /// A runner at the bottom of the stack that records which port methods reach it.
    ///
    /// The backstop for [`forwards_to_inner!`]. The macro stops a decorator forgetting to
    /// pass a method along; this proves the whole layered chain is actually transparent,
    /// including the runners that aggregate rather than wrap and so cannot use the macro.
    #[derive(Default)]
    struct RecordsWhatReachesIt {
        reached: std::sync::Mutex<std::collections::BTreeSet<&'static str>>,
    }

    impl RecordsWhatReachesIt {
        fn note(&self, method: &'static str) {
            self.reached.lock().unwrap().insert(method);
        }
    }

    /// The one skill the recorder claims. `CompositeRunner` routes by **which child offers
    /// the id**, so a recorder that offers nothing is never asked anything — the first run
    /// of this test failed that way and the setup was at fault, not the code.
    const A_SKILL_IT_OFFERS: &str = "recorder.thing";

    impl CapabilityRunner for RecordsWhatReachesIt {
        fn available(&self) -> Vec<crate::application::CapabilitySpec> {
            vec![crate::application::CapabilitySpec {
                id: A_SKILL_IT_OFFERS.to_owned(),
                description: String::new(),
                configured: true,
                autonomous: true,
                reversibility: Reversibility::Observe,
                input_schema: None,
            }]
        }

        fn run(&self, _id: &str, _input_json: &str) -> Result<String, String> {
            Ok(String::new())
        }

        fn about_the_person(&self) -> Vec<String> {
            self.note("about_the_person");
            Vec::new()
        }

        fn current_states(&self) -> Vec<(String, String)> {
            self.note("current_states");
            Vec::new()
        }

        fn decision(&self, _id: &str) -> Option<Decision> {
            self.note("decision");
            Some(Decision::Act)
        }

        fn verifier(&self, _id: &str) -> Option<String> {
            self.note("verifier");
            None
        }

        fn read_back_input(&self, _action_id: &str, _action_input: &str) -> String {
            self.note("read_back_input");
            String::new()
        }
    }

    /// Every method of the port that has a **default**, and so can be dropped silently.
    ///
    /// `available` and `run` are deliberately absent: they are required methods, so the
    /// compiler already refuses to let a decorator forget them. These five are the whole
    /// risk surface, and both real occurrences of this bug were among them.
    const CAN_BE_DROPPED_SILENTLY: [&str; 5] = [
        "about_the_person",
        "current_states",
        "decision",
        "read_back_input",
        "verifier",
    ];

    #[test]
    fn every_defaultable_method_survives_the_wrapper_chain() {
        // The mechanism version of the test below. That one names two methods, which means
        // it only ever catches the two bugs that already happened; this one asserts the
        // chain is transparent for the whole risk surface, so a sixth defaultable method
        // added to the port is covered the moment it is listed in one place.
        let bottom = Arc::new(RecordsWhatReachesIt::default());
        let chain: Arc<dyn CapabilityRunner + Send + Sync> =
            Arc::new(ReversibleOnlyRunner::new(Arc::new(WithdrawnRunner::new(
                Arc::new(CompositeRunner::new(vec![Arc::new(AliasRunner::new(
                    Arc::new(OpenerRunner::new(
                        Arc::clone(&bottom) as Arc<dyn CapabilityRunner + Send + Sync>,
                        std::collections::HashMap::new(),
                        std::collections::HashSet::new(),
                        false,
                    )),
                    Vec::new(),
                ))
                    as Arc<dyn CapabilityRunner + Send + Sync>]))
                    as Arc<dyn CapabilityRunner + Send + Sync>,
                std::collections::HashSet::new(),
            ))));

        // Call each one at the TOP, the only place production ever calls them.
        let _ = chain.about_the_person();
        let _ = chain.current_states();
        let _ = chain.decision(A_SKILL_IT_OFFERS);
        let _ = chain.verifier(A_SKILL_IT_OFFERS);
        let _ = chain.read_back_input(A_SKILL_IT_OFFERS, "{}");

        let reached = bottom.reached.lock().unwrap().clone();
        let missing: Vec<&str> = CAN_BE_DROPPED_SILENTLY
            .iter()
            .copied()
            .filter(|m| !reached.contains(m))
            .collect();
        assert!(
            missing.is_empty(),
            "these never reached the bottom of the stack, so a service answering them \
             would be silently ignored: {missing:?}"
        );
    }

    #[test]
    fn what_the_services_know_survives_the_whole_stack() {
        // The bug this exists for: `about_the_person` and `current_states` are answered by
        // the channel runner in the middle of the stack, and every wrapper above it has to
        // pass them through. Two did not — so presence never reached a turn and neither
        // did the facts behind an answer, while the unit tests passed because they poked
        // the inner runner directly.
        //
        // A defaulted port method that returns nothing is indistinguishable from a service
        // having nothing to say, which is what made it silent. This asserts through the
        // composed stack, the way production builds it.
        let (_, inner) = with_direct();
        let composed = ReversibleOnlyRunner::new(Arc::new(WithdrawnRunner::new(
            Arc::new(CompositeRunner::new(vec![
                Arc::new(inner) as Arc<dyn CapabilityRunner + Send + Sync>
            ])) as Arc<dyn CapabilityRunner + Send + Sync>,
            std::collections::HashSet::new(),
        )));
        assert_eq!(
            composed.about_the_person(),
            vec!["john is not home".to_owned()],
            "presence was dropped somewhere in the stack"
        );
        assert!(
            composed
                .current_states()
                .iter()
                .any(|(name, _)| name == "Kitchen Table"),
            "the facts behind an answer were dropped somewhere in the stack"
        );
    }
}

#[cfg(test)]
mod a_field_that_may_be_left_blank {
    use super::*;

    #[test]
    fn an_optional_setting_does_not_hold_a_skill_back() {
        // Live: the Home Assistant skill read "needs setup" and was left out of the model's
        // catalogue entirely, with a URL and a token both set — because `mcp_server` is
        // documented as "blank = home-assistant" and had never been filled in. A field the
        // interface itself describes as optional cannot also be the reason a skill is off.
        let info = HomeAssistantCapability.info();
        assert!(
            info.settings.iter().any(|s| s.optional),
            "this skill is the reason the flag exists"
        );

        let mut only_the_required: CapabilitySettings = CapabilitySettings::new();
        for spec in info.settings.iter().filter(|s| !s.optional) {
            only_the_required.insert(spec.key.to_owned(), "something".to_owned());
        }
        assert!(
            settings_complete(&info, &only_the_required),
            "filling in every REQUIRED field must be enough"
        );
    }

    #[test]
    fn a_required_setting_still_holds_it_back() {
        // The flag must not become a way for a skill to claim it is ready with nothing set.
        let info = HomeAssistantCapability.info();
        assert!(
            !settings_complete(&info, &CapabilitySettings::new()),
            "no URL and no token is not configured"
        );
    }
}

#[cfg(test)]
mod an_address_someone_typed {
    use super::as_url;

    #[test]
    fn a_bare_host_and_port_becomes_a_url() {
        // Live, and invisible: this is exactly what was in the settings, and every request
        // built from it failed with `http: invalid format` — taking presence, live states
        // and the standing-trouble watch down without anything appearing to be broken.
        assert_eq!(as_url("192.168.1.10:8123"), "http://192.168.1.10:8123");
        assert_eq!(
            as_url("  homeassistant.local:8123  "),
            "http://homeassistant.local:8123"
        );
    }

    #[test]
    fn a_scheme_someone_gave_is_never_overridden() {
        // Someone who needs TLS has typed why, and guessing over them would be worse than
        // the bug this fixes.
        assert_eq!(as_url("https://ha.example.com"), "https://ha.example.com");
        assert_eq!(
            as_url("http://192.168.1.10:8123/"),
            "http://192.168.1.10:8123"
        );
    }

    #[test]
    fn nothing_typed_stays_nothing() {
        // An unset address must remain unset, so "you have not configured this" keeps
        // being the message rather than a request to `http://`.
        assert_eq!(as_url(""), "");
        assert_eq!(as_url("   "), "");
    }
}

#[cfg(test)]
mod pressing_test_is_safe {
    use super::*;

    /// A skill that can change the world, and records whether it was run.
    struct Actuator(std::sync::atomic::AtomicBool);
    impl Capability for Actuator {
        fn info(&self) -> CapabilityInfo {
            CapabilityInfo {
                id: "wire_transfer",
                name: "Wire transfer",
                description: "",
                category: "",
                reaches_external: true,
                reversibility: Reversibility::Irreversible,
                configured: true,
                needs: "",
                settings: &[],
                input_schema: None,
            }
        }
        fn invoke(
            &self,
            _input: &Value,
            _settings: &CapabilitySettings,
        ) -> Result<Value, CapabilityError> {
            self.0.store(true, std::sync::atomic::Ordering::SeqCst);
            Ok(Value::Null)
        }
    }

    /// A read-only skill, which proves itself by running.
    struct Reader;
    impl Capability for Reader {
        fn info(&self) -> CapabilityInfo {
            CapabilityInfo {
                id: "weather",
                name: "Weather",
                description: "",
                category: "",
                reaches_external: true,
                reversibility: Reversibility::Observe,
                configured: true,
                needs: "",
                settings: &[],
                input_schema: None,
            }
        }
        fn invoke(
            &self,
            _input: &Value,
            _settings: &CapabilitySettings,
        ) -> Result<Value, CapabilityError> {
            Ok(serde_json::json!({ "temp": 72 }))
        }
        fn summarize(&self, _o: &Value) -> String {
            "72F, sunny".to_owned()
        }
    }

    #[test]
    fn a_skill_that_can_act_is_never_run_to_test_it() {
        // "Press this to find out" must never be how someone discovers what a skill does.
        // The wire transfer is the deliberately absurd case, and the rule is the same for
        // every actuator: turning a light on to prove the light skill works is still an
        // action nobody asked for.
        let actuator = Actuator(std::sync::atomic::AtomicBool::new(false));
        let answer = actuator.self_test(&CapabilitySettings::default());

        assert!(answer.is_err(), "an actuator must refuse: {answer:?}");
        assert!(
            !actuator.0.load(std::sync::atomic::Ordering::SeqCst),
            "it RAN — a test button that transfers money is worse than no test button"
        );
    }

    #[test]
    fn a_read_only_skill_proves_itself_by_running() {
        // The default is not a stub: every observing skill gets a working test without
        // anyone writing one.
        assert_eq!(
            Reader.self_test(&CapabilitySettings::default()).unwrap(),
            "72F, sunny"
        );
    }

    #[test]
    fn a_skill_that_cannot_reach_its_service_says_so_rather_than_claiming_to_work() {
        // Home Assistant with no URL configured — the exact case the button exists for.
        let answer = HomeAssistantCapability.self_test(&CapabilitySettings::default());
        let Err(CapabilityError::Unavailable(why)) = answer else {
            panic!("expected an unavailable answer, got {answer:?}");
        };
        assert!(why.contains("URL"), "{why}");
    }
}

#[cfg(test)]
mod what_a_thing_is_actually_telling_you {
    use super::facts_worth_reading;
    use serde_json::json;

    #[test]
    fn a_calendars_meaning_is_entirely_in_its_attributes() {
        // Captured from a live house. Connecting a calendar achieved nothing until this
        // existed: the state is `off`, and the evening is in the attributes.
        let family = json!({
            "message": "Jane Doe & John Doe",
            "all_day": false,
            "start_time": "2026-07-31 18:30:00",
            "end_time": "2026-07-31 19:30:00",
            "location": "",
            "description": "-::~:~::~:~:~:~:~:~:~:~:~:~:~:~:~:~:~:~:~:~:~:~:~:~:~:~:~:~:~:~:~:~:~:~:~:~::~:~::-",
            "friendly_name": "Family",
            "supported_features": 1
        });
        let facts = facts_worth_reading(&family);
        assert_eq!(facts["message"], "Jane Doe & John Doe");
        assert_eq!(facts["start_time"], "2026-07-31 18:30:00");
        // An empty location is not a fact about the evening.
        assert!(!facts.contains_key("location"));
        // Boilerplate the service attaches to every reading.
        assert!(
            !facts.contains_key("description"),
            "500 characters of tildes"
        );
        assert!(!facts.contains_key("friendly_name"), "already the name");
        assert!(!facts.contains_key("supported_features"), "a bitmask");
    }

    #[test]
    fn the_weather_is_a_temperature_not_a_word() {
        // `clear-night` is the whole state. The temperature is an attribute, which is why
        // a butler with a working weather entity still could not say how warm it was.
        let forecast = json!({
            "temperature": 73, "humidity": 72, "temperature_unit": "°F",
            "attribution": "Weather forecast from met.no, delivered by the Norwegian Meteorological Institute",
            "friendly_name": "Forecast Home", "supported_features": 3
        });
        let facts = facts_worth_reading(&forecast);
        assert_eq!(facts["temperature"], 73);
        assert_eq!(facts["temperature_unit"], "°F");
        assert!(!facts.contains_key("attribution"), "boilerplate, and long");
    }

    #[test]
    fn a_list_is_never_a_fact_about_the_thing() {
        // An Apple TV's `source_list` is forty app names. Including it would put a page of
        // noise beside every media player, on every turn that reads the house.
        let apple_tv = json!({
            "source_list": ["App Store", "Arcade", "Channels", "Computers", "Crunchyroll"],
            "friendly_name": "Apple TV", "supported_features": 450_487
        });
        assert!(
            facts_worth_reading(&apple_tv).is_empty(),
            "nothing to say while it is off"
        );
    }

    #[test]
    fn nothing_discursive_gets_in_even_bounded() {
        // Dropped rather than truncated: sixty characters of a paragraph is still sixty
        // characters of nothing, and it costs the same as sixty useful ones.
        let wordy = json!({ "note": "x".repeat(400) });
        assert!(facts_worth_reading(&wordy).is_empty());
    }
}

#[cfg(test)]
mod the_seam_a_local_integration_plugs_into {
    //! Registration for native channels (ADR 0050/0054).
    //!
    //! The node used to build the one existing channel by name, in the composition root, with
    //! an early return when its settings were missing — so a second local integration could
    //! not have been reached even after somebody wrote it. These are the tests that could not
    //! be written against that shape.

    use super::{
        Capability, CapabilityError, CapabilityInfo, CapabilitySettings, NativeChannel, channels_of,
    };
    use crate::domain::TargetAlias;
    use endora_kernel::Reversibility;
    use serde_json::Value;
    use std::sync::Arc;

    /// A channel that remembers which names it was told about, so the test can see that the
    /// right aliases reached the right integration.
    struct Speaks(Vec<String>);
    impl NativeChannel for Speaks {
        fn known(&self) -> Result<Vec<(String, String)>, String> {
            Ok(self.0.iter().map(|n| (n.clone(), n.clone())).collect())
        }
        fn reading(&self) -> Result<String, String> {
            Ok(String::new())
        }
        fn act(&self, _tool: &str, _id: &str) -> Option<Result<String, String>> {
            None
        }
    }

    /// A skill that speaks for a service, once it has been configured.
    struct Local {
        id: &'static str,
        server: &'static str,
    }

    impl Capability for Local {
        fn info(&self) -> CapabilityInfo {
            CapabilityInfo {
                id: self.id,
                name: "a local thing",
                description: "",
                category: "presence",
                reaches_external: true,
                reversibility: Reversibility::Observe,
                configured: true,
                needs: "",
                settings: &[],
                input_schema: None,
            }
        }
        fn invoke(
            &self,
            _input: &Value,
            _settings: &CapabilitySettings,
        ) -> Result<Value, CapabilityError> {
            Ok(Value::Null)
        }
        fn channel(
            &self,
            settings: &CapabilitySettings,
            aliases: &[TargetAlias],
        ) -> Option<(String, Arc<dyn NativeChannel>)> {
            // Not configured is the integration's own judgement, not the caller's.
            settings.get("url")?;
            let mine = aliases
                .iter()
                .filter(|a| a.server == self.server)
                .map(|a| a.said.clone())
                .collect();
            Some((self.server.to_owned(), Arc::new(Speaks(mine))))
        }
    }

    /// A perfectly ordinary skill: it answers questions and watches nothing.
    struct JustAnswers;
    impl Capability for JustAnswers {
        fn info(&self) -> CapabilityInfo {
            CapabilityInfo {
                id: "just_answers",
                name: "answers",
                description: "",
                category: "information",
                reaches_external: true,
                reversibility: Reversibility::Observe,
                configured: true,
                needs: "",
                settings: &[],
                input_schema: None,
            }
        }
        fn invoke(
            &self,
            _input: &Value,
            _settings: &CapabilitySettings,
        ) -> Result<Value, CapabilityError> {
            Ok(Value::Null)
        }
    }

    fn configured(id: &str) -> (String, CapabilitySettings) {
        let mut s = CapabilitySettings::default();
        s.insert("url".to_owned(), "http://x".to_owned());
        (id.to_owned(), s)
    }

    #[test]
    fn a_second_local_integration_is_actually_reached() {
        // The regression this refactor exists for. Under the old shape the first unconfigured
        // integration returned an empty list for everybody, so this could not have passed —
        // and nobody would have found out until they wrote the second one.
        let skills: Vec<Arc<dyn Capability>> = vec![
            Arc::new(Local {
                id: "one",
                server: "server-one",
            }),
            Arc::new(JustAnswers),
            Arc::new(Local {
                id: "two",
                server: "server-two",
            }),
        ];
        let settings = [configured("one"), configured("two")].into_iter().collect();

        let found: Vec<String> = channels_of(&skills, &settings, &[])
            .into_iter()
            .map(|(server, _)| server)
            .collect();
        assert_eq!(found, vec!["server-one", "server-two"]);
    }

    #[test]
    fn one_unconfigured_integration_does_not_silence_the_others() {
        // Precisely the old bug: `settings.get(..) else { return Vec::new() }` meant the first
        // miss ended registration for everything behind it.
        let skills: Vec<Arc<dyn Capability>> = vec![
            Arc::new(Local {
                id: "not_set_up",
                server: "quiet",
            }),
            Arc::new(Local {
                id: "ready",
                server: "loud",
            }),
        ];
        let settings = [configured("ready")].into_iter().collect();

        let found: Vec<String> = channels_of(&skills, &settings, &[])
            .into_iter()
            .map(|(server, _)| server)
            .collect();
        assert_eq!(found, vec!["loud"]);
    }

    #[test]
    fn a_skill_that_only_answers_questions_supplies_nothing() {
        // The default, and the ordinary case: almost every skill wants nothing to do with this.
        let skills: Vec<Arc<dyn Capability>> = vec![Arc::new(JustAnswers)];
        assert!(channels_of(&skills, &std::collections::HashMap::new(), &[]).is_empty());
    }

    #[test]
    fn each_integration_is_handed_every_alias_and_keeps_its_own() {
        // Filtering belongs to the integration: which confirmed names are its own is knowledge
        // only it has, and deciding out here would put a per-integration branch back into
        // shared registration code (ADR 0054).
        let skills: Vec<Arc<dyn Capability>> = vec![
            Arc::new(Local {
                id: "one",
                server: "server-one",
            }),
            Arc::new(Local {
                id: "two",
                server: "server-two",
            }),
        ];
        let settings = [configured("one"), configured("two")].into_iter().collect();
        let aliases = vec![
            TargetAlias::new("server-one", "the lamp", "Living Room Lamp").unwrap(),
            TargetAlias::new("server-two", "the panel", "Alarm").unwrap(),
        ];

        let channels = channels_of(&skills, &settings, &aliases);
        let names_of = |i: usize| -> Vec<String> {
            channels[i]
                .1
                .known()
                .unwrap()
                .into_iter()
                .map(|(said, _)| said)
                .collect()
        };
        assert_eq!(names_of(0), vec!["the lamp"]);
        assert_eq!(names_of(1), vec!["the panel"]);
    }
}

#[cfg(test)]
mod a_standing_default_never_overwrites_a_decision {
    //! `trust_all` on connect (ADR 0054 — confirmed beats declared).
    //!
    //! Live: voice broadcast was blocked at the person's request and came back on, because
    //! connect re-opened every tool on the trusted server at every start-up. Four deploys in
    //! one afternoon restored a capability they had said no to, and nothing announced it.

    use super::tools_to_open_on_connect;

    fn ids(all: &[&str]) -> Vec<String> {
        all.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn a_tool_the_person_blocked_is_never_reopened() {
        // The bug, exactly. `HassBroadcast` has a stored decision, so connect leaves it alone
        // however many times the node restarts.
        let open_these = tools_to_open_on_connect(
            &ids(&["house.HassTurnOn", "house.HassBroadcast"]),
            &ids(&["house."]),
            &ids(&["house.HassBroadcast"]),
        );
        assert_eq!(open_these, vec!["house.HassTurnOn"]);
    }

    #[test]
    fn a_tool_the_person_allowed_is_left_alone_too() {
        // Not just blocks. Anything already ruled on is theirs, and rewriting it to the same
        // value would still be the system overwriting a decision it did not make.
        assert!(
            tools_to_open_on_connect(
                &ids(&["house.HassTurnOn"]),
                &ids(&["house."]),
                &ids(&["house.HassTurnOn"]),
            )
            .is_empty()
        );
    }

    #[test]
    fn a_new_tool_on_a_trusted_server_is_still_opened() {
        // The point of the flag, and what must survive the fix: a server that grows a tool
        // should not need a click for it.
        let open_these = tools_to_open_on_connect(
            &ids(&["house.HassTurnOn", "house.HassBrandNew"]),
            &ids(&["house."]),
            &ids(&["house.HassTurnOn"]),
        );
        assert_eq!(open_these, vec!["house.HassBrandNew"]);
    }

    #[test]
    fn an_untrusted_server_is_not_touched_at_all() {
        assert!(
            tools_to_open_on_connect(&ids(&["elsewhere.DoAThing"]), &ids(&["house."]), &[],)
                .is_empty()
        );
    }

    /// The other half, and the one that was missing.
    ///
    /// Live: a freshly added search server read "Allow all its tools: On" with all eight of
    /// its tools blocked, because the only thing the on direction did was store the flag and
    /// leave the opening to connect — which skips anything already ruled on.
    ///
    /// Connect is a default and must not overwrite a decision. The toggle **is** a decision.
    #[test]
    fn the_toggle_governs_every_tool_including_the_ruled_on_ones() {
        let governed = super::tools_the_toggle_governs(
            &ids(&["search.web", "search.news", "search.images"]),
            "search",
        );
        assert_eq!(governed, vec!["search.web", "search.news", "search.images"]);
    }

    #[test]
    fn the_toggle_stops_at_its_own_server() {
        // Same rule as the off direction, which already closed only its own server's tools.
        // A prefix that is a prefix of another name must not reach into it.
        let governed = super::tools_the_toggle_governs(
            &ids(&["search.web", "search-extra.web", "house.HassTurnOn"]),
            "search",
        );
        assert_eq!(governed, vec!["search.web"]);
    }
}

#[cfg(test)]
mod pressing_test_has_to_reach_the_service {
    //! Live: a Brave server listed eight tools, connected cleanly, reported no error — and
    //! was subscribed to the wrong API. A handshake never calls the service, so everything
    //! on the card looked healthy while nothing had been proven.

    #[test]
    fn a_test_call_fills_in_what_the_tool_requires() {
        let schema = r#"{"type":"object",
            "properties":{"query":{"type":"string"},"count":{"type":"integer"}},
            "required":["query"]}"#;
        // The required field is filled; the optional one is not invented.
        assert_eq!(
            super::arguments_for_a_test_call(Some(schema)),
            r#"{"query":"test"}"#
        );
    }

    #[test]
    fn a_test_call_takes_a_declared_choice_over_an_invented_one() {
        let schema = r#"{"type":"object",
            "properties":{"freshness":{"enum":["pd","pw","pm"]}},
            "required":["freshness"]}"#;
        assert_eq!(
            super::arguments_for_a_test_call(Some(schema)),
            r#"{"freshness":"pd"}"#
        );
    }

    #[test]
    fn a_test_call_without_a_schema_sends_nothing() {
        assert_eq!(super::arguments_for_a_test_call(None), "{}");
        assert_eq!(super::arguments_for_a_test_call(Some("not json")), "{}");
    }

    #[test]
    fn a_required_field_of_unstated_type_is_left_to_the_server_to_complain_about() {
        // Guessing produces a rejection that reads like a broken credential. The server's
        // own message is more use than an invented value.
        let schema = r#"{"type":"object","properties":{"thing":{}},"required":["thing"]}"#;
        assert_eq!(super::arguments_for_a_test_call(Some(schema)), "{}");
    }
}

#[cfg(test)]
mod every_skill_that_needs_arguments_says_so {
    //! The gap that bit twice in one afternoon, and the second time was worse.
    //!
    //! A skill that requires an argument and declares no schema is offered to the model as
    //! taking nothing. The venue skill was called with `{}` and answered "say what to look
    //! for", twice — visibly broken, and recoverable. `web_fetch` was called with `{}` too,
    //! and the model supplied `https://example.com`, which **succeeded**: it came back with
    //! "this domain is for use in documentation examples" and the butler answered from it.
    //!
    //! Fixing the one in front of me and not sweeping for the rest is what let the second
    //! one ship. This sweeps.

    use crate::{CapabilityError, CapabilitySettings};

    /// Called with no arguments and no settings, so nothing here reaches a network: a skill
    /// that needs either refuses before it would.
    #[test]
    fn a_skill_that_refuses_an_empty_call_has_declared_its_arguments() {
        let empty = CapabilitySettings::default();
        let mut offenders = Vec::new();
        for c in super::default_capabilities() {
            let info = c.info();
            // Irreversible skills are never invoked to find out what they do.
            if info.reversibility != endora_kernel::Reversibility::Observe {
                continue;
            }
            let refused_for_arguments = matches!(
                c.invoke(&serde_json::json!({}), &empty),
                Err(CapabilityError::BadInput(_))
            );
            if refused_for_arguments && info.input_schema.is_none() {
                offenders.push(info.id);
            }
        }
        assert!(
            offenders.is_empty(),
            "these need arguments and tell the model they need none, so it will guess — \
             and a guess that happens to work is worse than one that fails: {offenders:?}"
        );
    }
}

#[cfg(test)]
mod a_skill_can_say_what_it_takes {
    //! Live: a new skill that needs a venue was offered to the model with an empty parameter
    //! object — *this takes nothing* — so it called it with `{}` twice and was told "say what
    //! to look for" both times. It was doing exactly what it had been told it could.
    //!
    //! MCP tools have carried a real schema all along. The skills written here had no way to
    //! say, and the two that really take arguments got by on a hand-written example in the
    //! system prompt: a per-skill patch that only ever covered whoever was remembered.

    use crate::application::CapabilityRunner;
    use std::sync::Arc;

    #[test]
    fn a_skill_that_needs_arguments_hands_the_model_their_names() {
        let runner = super::RegistryRunner::new(Arc::new(super::default_capabilities()));
        let spec = runner
            .available()
            .into_iter()
            .find(|c| c.id == "ticketed_events")
            .expect("the skill is registered");
        let schema = spec
            .input_schema
            .expect("a skill that needs arguments must say so");
        let parsed: serde_json::Value =
            serde_json::from_str(&schema).expect("the schema must be JSON the model can read");
        let properties = parsed
            .get("properties")
            .and_then(serde_json::Value::as_object)
            .expect("a schema with no properties tells the model it takes nothing");
        assert!(properties.contains_key("what"), "{properties:?}");
        assert!(properties.contains_key("city"), "{properties:?}");
    }

    #[test]
    fn a_skill_that_takes_nothing_still_says_nothing() {
        // The default has to stay honest: an empty schema is right for a skill with no
        // arguments, and wrong only for one that has them.
        let runner = super::RegistryRunner::new(Arc::new(super::default_capabilities()));
        let spec = runner
            .available()
            .into_iter()
            .find(|c| c.id == "own_activity" || c.id == "city_meetings")
            .expect("some skill takes nothing");
        assert!(spec.input_schema.is_none());
    }
}

#[cfg(test)]
mod answers_worth_keeping {
    //! ADR 0061.
    //!
    //! The sources this runs on are free tiers, and a free tier is the budget. Asking the
    //! same question twice paid twice, so the butler could not afford to look things up
    //! often — and so it did not.

    use super::{fingerprint, keep_for_ms};

    /// The whole of an answer, live:
    ///
    /// > error: unavailable: io: invalid peer certificate: certificate expired:
    /// > verification time 1785717318 (UNIX), but certificate is not valid after
    /// > 1543938436 (241778882 seconds ago)
    ///
    /// Every word true, none of it an answer.
    #[test]
    fn a_transport_failure_is_said_the_way_a_person_would_say_it() {
        let raw = "io: invalid peer certificate: certificate expired: verification time \
                   1785717318 (UNIX), but certificate is not valid after 1543938436";
        let said = super::plainly(raw);
        assert!(said.contains("certificate has expired"), "{said}");
        assert!(
            !said.contains("1785717318"),
            "a unix timestamp reached the person: {said}"
        );
        assert!(!said.contains("io:"), "{said}");
    }

    #[test]
    fn an_unrecognised_failure_is_passed_through_rather_than_guessed_at() {
        // A wrong guess reads worse than a technical sentence, and unrecognised is exactly
        // where the next unknown failure lives.
        let odd = "the remote end did something nobody has seen before";
        assert_eq!(super::plainly(odd), odd);
    }

    #[test]
    fn the_source_decides_how_long_its_answer_is_good_for() {
        assert_eq!(keep_for_ms(Some("public, max-age=3600")), 3_600_000);
        // `s-maxage` wins where both are given: it is the one addressed to a shared cache,
        // which is what this is.
        assert_eq!(keep_for_ms(Some("max-age=60, s-maxage=1800")), 1_800_000);
    }

    #[test]
    fn a_source_is_not_owed_unlimited_trust_about_its_own_freshness() {
        // Claiming zero would make the whole thing pointless; the floor decides instead.
        assert_eq!(keep_for_ms(Some("max-age=0")), super::KEEP_AT_LEAST_MS);
        // Claiming a year would make the butler confidently out of date for a year.
        assert_eq!(
            keep_for_ms(Some("max-age=31536000")),
            super::KEEP_AT_MOST_MS
        );
        // Saying nothing at all is the common case, and gets the floor.
        assert_eq!(keep_for_ms(None), super::KEEP_AT_LEAST_MS);
        assert_eq!(keep_for_ms(Some("private")), super::KEEP_AT_LEAST_MS);
    }

    #[test]
    fn no_store_is_obeyed_exactly() {
        // The one instruction worth taking literally: the source is saying do not keep this.
        assert_eq!(keep_for_ms(Some("no-store")), 0);
        assert_eq!(keep_for_ms(Some("private, no-store, max-age=600")), 0);
    }

    /// The whole privacy posture of this record, as an assertion.
    ///
    /// A URL carries the API key of whatever is being asked and often the person's own town.
    /// What is kept must be enough to recognise the same question and not enough to rebuild
    /// it — so there is nothing here to redact in a log, because there is nothing to print.
    #[test]
    fn what_is_kept_cannot_be_turned_back_into_the_question() {
        let asked = "https://example.com/events?apikey=s3cr3t&city=Springfield";
        let (a, b) = fingerprint(asked);
        let held = format!("{a}{b}");
        assert!(!held.contains("s3cr3t"), "the credential survived");
        assert!(!held.contains("Springfield"), "the place survived");
        assert!(!held.contains("example.com"), "the source survived");
    }

    #[test]
    fn the_same_question_is_recognised_and_a_different_one_is_not() {
        let one = fingerprint("https://example.com/events?city=A");
        assert_eq!(one, fingerprint("https://example.com/events?city=A"));
        assert_ne!(one, fingerprint("https://example.com/events?city=B"));
        // Two independent hashes, so a collision is another question's answer rather than a
        // slow one — the halves must not be the same function twice.
        assert_ne!(one.0, one.1);
    }
}

#[cfg(test)]
mod a_list_said_like_a_person {
    use super::said_proportionately;

    #[test]
    fn few_are_named_and_many_are_shaped() {
        let few = vec!["a".to_owned(), "b".to_owned()];
        assert_eq!(said_proportionately(few, "on"), "a; b");
        let many: Vec<String> = (1..=7).map(|i| format!("thing {i}")).collect();
        let said = said_proportionately(many, "on");
        // The shape first, the first few, and the count of the rest — never the recital
        // a small model will read straight back to the person.
        assert!(said.starts_with("7 of them — thing 1;"), "{said}");
        assert!(said.ends_with("and 3 more"), "{said}");
        assert!(!said.contains("thing 5"), "{said}");
    }

    #[test]
    fn nothing_says_so() {
        assert_eq!(
            said_proportionately(Vec::new(), "on the calendar"),
            "Nothing on the calendar just now."
        );
    }
}

#[cfg(test)]
mod whats_on_at_a_venue {
    //! Reading Ticketmaster's Discovery API (ADR 0058 — answers, so MCP-shaped; a built-in
    //! skill because the only server offering it is a v0.1.0 remote gateway from an unknown
    //! publisher, and routing a credential and every query through a stranger costs more
    //! than the code it saves).
    //!
    //! Parsed rather than trusted, exactly as the civic agenda is. Field names are the whole
    //! contract with somebody else's service, and a renamed one must yield **no events**
    //! rather than an error or a half-built row.

    use super::{describe_ticketed_event, ticketed_events_in};

    /// Shaped as the documented response: events under `_embedded.events`, the venue under
    /// the event's own `_embedded`, and the time carrying seconds nobody says out loud.
    const ANSWERED: &str = r#"{"_embedded":{"events":[
        {"name":"Rovers vs Wanderers",
         "dates":{"start":{"localDate":"2026-08-04","localTime":"20:00:00"}},
         "priceRanges":[{"min":40.0,"max":220.0,"currency":"USD"}],
         "_embedded":{"venues":[{"name":"The Big Ground","city":{"name":"New York"}}]}},
        {"name":"An Evening With Somebody",
         "dates":{"start":{"localDate":"2026-08-07"}},
         "_embedded":{"venues":[{"name":"The Hall"}]}}
    ]}}"#;

    #[test]
    fn it_reads_what_is_on_and_where() {
        let on = ticketed_events_in(ANSWERED);
        assert_eq!(on.len(), 2);
        assert_eq!(on[0].what, "Rovers vs Wanderers");
        assert_eq!(on[0].on, "2026-08-04");
        // Seconds are a machine's way of saying eight o'clock.
        assert_eq!(on[0].at, "20:00");
        assert_eq!(on[0].place, "The Big Ground, New York");
        assert_eq!(on[0].from, "40");
    }

    #[test]
    fn a_listing_that_says_less_still_arrives() {
        // No time, no price, no town. All optional — only a name is load-bearing.
        let on = ticketed_events_in(ANSWERED);
        assert_eq!(on[1].at, "");
        assert_eq!(on[1].from, "");
        assert_eq!(on[1].place, "The Hall");
        assert_eq!(
            describe_ticketed_event(&on[1]),
            "An Evening With Somebody on 2026-08-07, The Hall"
        );
    }

    #[test]
    fn it_says_an_event_the_way_somebody_would() {
        let on = ticketed_events_in(ANSWERED);
        assert_eq!(
            describe_ticketed_event(&on[0]),
            "Rovers vs Wanderers on 2026-08-04 at 20:00, The Big Ground, New York, from $40"
        );
    }

    /// Nothing on is an answer. An error is not.
    #[test]
    fn an_empty_search_is_empty_rather_than_broken() {
        // What the API returns when a search matches nothing: no `_embedded` at all.
        assert!(ticketed_events_in(r#"{"page":{"totalElements":0}}"#).is_empty());
    }

    /// A key in a query string is a key in every error about that request.
    ///
    /// What an HTTP crate prints on failure is its choice, not ours, and "401 for
    /// https://…?apikey=REAL_KEY" would reach both the person and the record of what was
    /// tried. Removing it is a guarantee; hoping about a `Display` impl is not.
    #[test]
    fn a_failure_never_carries_the_key_back_out() {
        let leaked = "http status 401 for \
                      https://app.ticketmaster.com/discovery/v2/events.json?apikey=s3cr3t&size=20";
        let safe = super::without_the_key(leaked, "s3cr3t");
        assert!(!safe.contains("s3cr3t"), "{safe}");
        // And it still says what went wrong, which is the useful half.
        assert!(safe.contains("401"), "{safe}");
    }

    #[test]
    fn with_no_key_configured_a_message_is_left_alone() {
        // An empty needle would otherwise match everywhere and shred the message.
        assert_eq!(
            super::without_the_key("could not connect", "   "),
            "could not connect"
        );
    }

    /// The failure this skill exists to avoid is the quiet one.
    #[test]
    fn a_renamed_field_yields_nothing_rather_than_a_half_built_row() {
        let renamed = r#"{"_embedded":{"events":[
            {"title":"Rovers vs Wanderers","dates":{"start":{"localDate":"2026-08-04"}}}]}}"#;
        assert!(ticketed_events_in(renamed).is_empty());
        assert!(ticketed_events_in("not json at all").is_empty());
    }
}

#[cfg(test)]
mod what_the_city_is_doing {
    //! Reading Legistar (ADR 0058 — answers, so MCP-shaped, but it is a built-in skill because
    //! there is no server to run).
    //!
    //! Parsed rather than trusted. Field names are the only contract this has with somebody
    //! else's service, a renamed one yields empty meetings rather than an error, and **a screen
    //! quietly saying "nothing on" is the worst possible failure** for a thing whose entire job
    //! is to say what is on.

    use super::{describe_meeting, meetings_in, today_utc};

    /// Shaped exactly as the live API answered on 2026-08-02, including the zeroed time in
    /// `EventDate` that a naive read would show somebody as midnight.
    const REAL: &str = r#"[
      {"EventId":1,"EventDate":"2026-08-03T00:00:00","EventTime":"9:00 AM",
       "EventBodyName":"Housing Council Committee",
       "EventLocation":"City Government Center, Room 267"},
      {"EventId":2,"EventDate":"2026-08-03T00:00:00","EventTime":"5:00 PM",
       "EventBodyName":"Transportation, Planning, and Development Council Committee ",
       "EventLocation":""}
    ]"#;

    #[test]
    fn it_reads_the_shape_the_service_actually_returns() {
        let found = meetings_in(REAL);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].who, "Housing Council Committee");
        assert_eq!(found[0].at, "9:00 AM");
        assert_eq!(
            found[0].on, "2026-08-03",
            "the zeroed time must not reach a person as midnight"
        );
    }

    #[test]
    fn a_trailing_space_in_a_committee_name_is_not_part_of_the_name() {
        // The live answer has one. Left in, it reads as a typo somebody will assume is ours.
        assert_eq!(
            meetings_in(REAL)[1].who,
            "Transportation, Planning, and Development Council Committee"
        );
    }

    #[test]
    fn a_meeting_nobody_can_name_is_not_reported() {
        let body = r#"[{"EventId":1,"EventDate":"2026-08-03T00:00:00","EventBodyName":""}]"#;
        assert!(meetings_in(body).is_empty());
    }

    #[test]
    fn everything_but_the_name_is_allowed_to_be_missing() {
        // A meeting with no time or place is still a meeting, and dropping it would be
        // silently answering "nothing on" when something is.
        let body = r#"[{"EventId":1,"EventBodyName":"City Council Business Meeting"}]"#;
        let found = meetings_in(body);
        assert_eq!(found.len(), 1);
        assert_eq!(describe_meeting(&found[0]), "City Council Business Meeting");
    }

    #[test]
    fn an_answer_this_cannot_read_is_no_meetings_rather_than_a_crash() {
        for body in ["", "not json", "{}", r#"{"error":"nope"}"#, "[]"] {
            assert!(meetings_in(body).is_empty(), "{body:?}");
        }
    }

    #[test]
    fn a_meeting_is_said_the_way_somebody_would_mention_it() {
        let found = meetings_in(REAL);
        assert_eq!(
            describe_meeting(&found[0]),
            "Housing Council Committee on 2026-08-03 at 9:00 AM, \
             City Government Center, Room 267"
        );
        assert_eq!(
            describe_meeting(&found[1]),
            "Transportation, Planning, and Development Council Committee on 2026-08-03 at 5:00 PM"
        );
    }

    #[test]
    fn today_is_the_shape_the_query_needs() {
        // The filter is a string comparison in somebody else's query language; a wrong shape
        // returns everything since 2015 rather than failing.
        let today = today_utc();
        assert_eq!(today.len(), 10, "{today}");
        assert_eq!(today.matches('-').count(), 2, "{today}");
        let (y, rest) = today.split_at(4);
        assert!(
            y.parse::<u32>().is_ok_and(|y| (2020..2200).contains(&y)),
            "{today}"
        );
        assert!(rest.starts_with('-'));
    }
}
