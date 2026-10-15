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

/// Read-only, explicitly configured home-lab dashboard adapters.
pub mod homelab;
/// Chat-facing reads over the handheld's configured services (read-only; see ADR 0093).
pub mod homelab_chat;
/// Explicit, fixed-route handheld administration adapters.
pub mod terminal;

use crate::application::{CapabilityRunner, Recipe, Stance};
use crate::domain::{RecipeInput, RecipeInputKind, placeholders_in};
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
    /// Whether this skill needs a place, and should be given the person's when the request
    /// does not name one.
    ///
    /// Three fixes in two days tried to make the model more likely to remember where
    /// somebody lives — take the example city out of the prompt, state the place as ground
    /// truth, then fix the parser that made the statement empty. Each one shipped, each one
    /// was verified, and the fourth brief still opened "here's your daily brief for New
    /// York". A 7B model reading twelve messages, three of which name a city it once said,
    /// will keep saying it.
    ///
    /// So it is not asked. A place the person did not name is filled in from what they told
    /// Endora, in code, before the skill runs. The model can still name a different place —
    /// "the weather in Boston" is a real request — and it can no longer get *home* wrong,
    /// because home was never its to remember.
    pub wants_place: bool,

    /// Another capability whose settings this one uses, when it rides an existing
    /// connection rather than asking for its own.
    ///
    /// Settings are keyed by a capability's own id, so a skill that borrows a connection and
    /// declares nothing gets an empty map — and reports itself unconfigured while the
    /// connection it wanted is right there. Declaring the same settings again would work and
    /// would make the person paste the same token twice, which is how two copies of a
    /// credential start.
    ///
    /// Named, not guessed, and general: any skill that rides another's connection says so
    /// and nothing in shared code learns an integration by name.
    pub borrows_from: Option<&'static str>,

    /// Whether what this returns is **prose somebody else wrote** (ADR 0064).
    ///
    /// A web page, a search result, a news item, a public agenda, a ticket listing — and,
    /// when it arrives, mail. Once any of it enters a turn, every actuator in that turn
    /// drops to confirm: an attacker may make the butler say something wrong, never do
    /// something.
    ///
    /// **The house is not a stranger.** A light reporting `on` is a reading, not an
    /// utterance, and machine state cannot carry an instruction. Declared rather than
    /// guessed from an id, so a skill written next year is covered by saying so.
    pub third_party: bool,

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
        // Whatever it needs to be asked, taken from its own schema, so a skill that wants a
        // query is tested with one (it used to be called with nothing, and "Test" on a
        // working search key answered "missing 'query'").
        let input =
            serde_json::from_str::<Value>(&arguments_for_a_test_call(self.info().input_schema))
                .unwrap_or_else(|_| Value::Object(serde_json::Map::new()));
        let out = self.invoke(&input, settings)?;
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
    let mut all: Vec<Arc<dyn Capability>> = vec![
        Arc::new(WeatherCapability),
        Arc::new(WebSearchCapability),
        Arc::new(WebFetchCapability),
        Arc::new(KnowledgeCapability),
        Arc::new(LocalNewsCapability),
        Arc::new(SportsScheduleCapability),
        Arc::new(ImageReviewCapability::from_env()),
        Arc::new(MailCapability),
        Arc::new(TrafficCapability),
        Arc::new(CityMeetingsCapability),
        Arc::new(TicketedEventsCapability),
        Arc::new(FlightSearchCapability),
        Arc::new(LocationLogCapability),
        Arc::new(SafetyAlertsCapability),
        Arc::new(IncidentScannerCapability),
        Arc::new(HomeAssistantCapability),
    ];
    all.extend(terminal::handheld_sources());
    // The reads of those same services, for chat: each borrows the settings entry above.
    all.extend(homelab_chat::chat_skills());
    all
}
mod sports;
use sports::*;

mod net;
pub use net::*;

mod skills;
pub use skills::*;

mod home;
pub(crate) use home::*;

mod registry;
pub use registry::*;

mod mcp;
pub use mcp::*;

mod recipes;
pub use recipes::*;

mod wrappers;
pub use wrappers::*;

mod channels;
pub use channels::*;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod a_field_that_may_be_left_blank;

#[cfg(test)]
mod an_address_someone_typed;

#[cfg(test)]
mod pressing_test_is_safe;

#[cfg(test)]
mod what_a_thing_is_actually_telling_you;

#[cfg(test)]
mod the_seam_a_local_integration_plugs_into;

#[cfg(test)]
mod a_standing_default_never_overwrites_a_decision;

#[cfg(test)]
mod pressing_test_has_to_reach_the_service;

#[cfg(test)]
mod every_skill_that_needs_arguments_says_so;

#[cfg(test)]
mod a_skill_can_say_what_it_takes;

#[cfg(test)]
mod answers_worth_keeping;

#[cfg(test)]
mod a_skill_may_ride_anothers_connection;

#[cfg(test)]
mod a_list_said_like_a_person;

#[cfg(test)]
mod whats_on_at_a_venue;

#[cfg(test)]
mod what_the_city_is_doing;

#[cfg(test)]
mod recipe_tests;
