//! Capabilities domain model — the autonomy envelope and the MCP server registry.

use endora_kernel::DomainError;
use endora_kernel::error::require_non_empty;

/// A change Endora made to a **service's own configuration**, kept so it can be put back
/// (ADR 0054).
///
/// ADR 0054 captured the prior value at the moment of the write and then dropped it on the
/// floor: the undo existed for the length of one function call. A record nobody stored is
/// not a reversibility story, it is a claim about one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigWrite {
    /// Monotonic id, so a write can be pointed at.
    pub id: u128,
    /// When it happened, in milliseconds since the epoch.
    pub at_ms: i64,
    /// Which service was edited.
    pub server: String,
    /// What in that service — an entity id, a path, whatever the service calls a thing.
    pub target: String,
    /// The name that was added.
    pub added: String,
    /// Every name it had **before**. Replaying this is the undo.
    pub was: Vec<String>,
    /// Whether it has since been put back.
    pub undone: bool,
    /// What sort of change this was, because undoing them differs.
    pub kind: WriteKind,
}

/// The sorts of change Endora makes to a service's configuration (ADR 0054).
///
/// Stored rather than derived. Add-versus-remove can be read off the prior value, but
/// these are different *acts* with different undos, and guessing would be dangerous: a
/// collection is created with no prior value, which reads exactly like adding a name —
/// so undoing it as one would strip every name off the thing it points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteKind {
    /// Another name for something that already exists. Undone by putting the old names
    /// back.
    Name,
    /// A new thing standing for several existing ones. Undone by removing it.
    Collection,
    /// Something taken out of the service's own view, because the person said it is gone.
    /// Undone by showing it again.
    Hidden,
}

impl WriteKind {
    /// How it is stored.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Collection => "collection",
            Self::Hidden => "hidden",
        }
    }

    /// Reads one back, defaulting to a name — which is what every row written before
    /// collections existed is.
    #[must_use]
    pub fn read(raw: &str) -> Self {
        if raw == "collection" {
            Self::Collection
        } else if raw == "hidden" {
            Self::Hidden
        } else {
            Self::Name
        }
    }
}

impl ConfigWrite {
    /// Whether this write **removed** the name rather than adding it.
    ///
    /// Derived, within a name change: a name already in the prior list can only have been
    /// taken away. That keeps one table and one undo for both directions.
    ///
    /// A collection is never a removal — it has no prior value to be in — which is why
    /// the *kind* is stored even though this is not.
    #[must_use]
    pub fn is_removal(&self) -> bool {
        self.kind == WriteKind::Name && self.was.iter().any(|a| a.eq_ignore_ascii_case(&self.added))
    }

    /// A one-line account of what changed, for the trail and the console.
    #[must_use]
    pub fn describe(&self) -> String {
        if self.kind == WriteKind::Collection {
            return format!("{} now stands for {}", self.added, self.target);
        }
        if self.is_removal() {
            return format!("{} no longer answers to '{}'", self.target, self.added);
        }
        let before = if self.was.is_empty() {
            "no other names".to_owned()
        } else {
            self.was.join(", ")
        };
        format!(
            "{} now also answers to '{}' (it was: {before})",
            self.target, self.added
        )
    }
}

/// A declared type for one [`Recipe`] input — the only two shapes a URL template
/// slot or a say-template value can be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecipeInputKind {
    /// Free text, URL-encoded when it fills a `get` slot.
    Text,
    /// A number, written with Rust's default float/int formatting.
    Number,
}

impl RecipeInputKind {
    /// A stable, lowercase name for storage and interfaces.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Text => "string",
            Self::Number => "number",
        }
    }

    /// Reads the stored name back. `None` for anything else — an unrecognised
    /// kind is not silently treated as text.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "string" => Some(Self::Text),
            "number" => Some(Self::Number),
            _ => None,
        }
    }
}

/// One named, typed input a [`Recipe`] declares — what may fill its templates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecipeInput {
    /// The slot's name, as it appears in `{name}` inside the templates.
    pub name: String,
    /// What shape a value for it must be.
    pub kind: RecipeInputKind,
}

/// A person-authored capability that is data, not code (ADR 0071).
///
/// Five fields are the whole expressiveness ceiling, and the ceiling **is** the
/// sandbox: a `get` template whose `{slot}` placeholders are filled only from
/// `inputs`, each value URL-encoded by the interpreter — a format with no way to
/// write an unescaped placeholder has no escaping to fail; and a `say` template
/// whose `{path.into.the.response}` placeholders are JSON paths, stringified, with
/// no expressions and no second request. Anything a recipe cannot say, it cannot
/// do.
///
/// Every placeholder in `get` must name a declared input — a template that could
/// never be filled is rejected at authoring time, not discovered as a 400 the
/// first time somebody enables it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recipe {
    /// A short, url-safe name — becomes `recipe.{id}` in the capability catalogue.
    pub id: String,
    /// What it does, in the person's or the butler's own words.
    pub description: String,
    /// What it accepts.
    pub inputs: Vec<RecipeInput>,
    /// The GET template. Every `{slot}` here must be a declared input's name.
    pub get: String,
    /// The template for what to say about the response — JSON-path placeholders.
    pub say: String,
}

impl Recipe {
    /// Builds a recipe, or refuses one whose template could never run.
    ///
    /// # Errors
    /// [`DomainError::EmptyField`] on a blank required field;
    /// [`DomainError::Malformed`] on an id with unsafe characters, a `get` whose
    /// scheme is not `http`/`https`, or a `get` placeholder naming no declared
    /// input.
    pub fn new(
        id: &str,
        description: &str,
        inputs: Vec<RecipeInput>,
        get: &str,
        say: &str,
    ) -> Result<Self, DomainError> {
        let id = require_non_empty("recipe.id", id)?;
        if !id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
            || !id.chars().next().is_some_and(|c| c.is_ascii_lowercase())
        {
            return Err(DomainError::Malformed {
                field: "recipe.id",
                reason: "must start with a lowercase letter and hold only \
                         lowercase letters, digits, and underscores",
            });
        }
        let description = require_non_empty("recipe.description", description)?;
        let get = require_non_empty("recipe.get", get)?;
        if !(get.starts_with("https://") || get.starts_with("http://")) {
            return Err(DomainError::Malformed {
                field: "recipe.get",
                reason: "must start with http:// or https:// — a recipe has no \
                         other way to reach anything",
            });
        }
        let say = require_non_empty("recipe.say", say)?;
        let declared: std::collections::HashSet<&str> =
            inputs.iter().map(|i| i.name.as_str()).collect();
        for slot in placeholders_in(&get) {
            if !declared.contains(slot) {
                return Err(DomainError::Malformed {
                    field: "recipe.get",
                    reason: "names a slot that is not a declared input",
                });
            }
        }
        Ok(Self {
            id,
            description,
            inputs,
            get,
            say,
        })
    }

    /// The id this recipe occupies in the capability catalogue.
    #[must_use]
    pub fn capability_id(&self) -> String {
        format!("recipe.{}", self.id)
    }
}

/// Every `{name}` placeholder in a template, in order of appearance, names only —
/// braces stripped. Shared by validation (every `get` slot must be declared) and
/// the interpreter (every `get`/`say` slot gets the same extraction).
#[must_use]
pub fn placeholders_in(template: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        let Some(close) = rest[open..].find('}') else {
            break;
        };
        out.push(&rest[open + 1..open + close]);
        rest = &rest[open + close + 1..];
    }
    out
}

/// Something in a service that has been wrong long enough to be worth saying (ADR 0056).
///
/// A butler that reports "13 entities unavailable" has added an item to someone's day. One
/// that says "these three have not answered since Tuesday — gone, or shall I hide them?"
/// has removed one. The difference is not the observation; it is having watched long
/// enough to say *since when*, and having somewhere for the answer to go.
///
/// **It cannot accumulate.** A row exists only while the trouble is still true: the moment
/// the thing answers again, the row is deleted. The store is bounded by what is currently
/// wrong, never by how long Endora has been running — which is what stops this becoming
/// the queue [0052](0052-what-it-knows-about-you.md) deleted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StandingTrouble {
    /// Which service it is in.
    pub server: String,
    /// What the service calls it.
    pub thing: String,
    /// What is wrong, in the service's own word for it — `unavailable`, `unknown`.
    pub trouble: String,
    /// When Endora first saw it this way. The whole point: without it there is no
    /// "since Tuesday", and without that there is no problem statement, only a reading.
    pub since_ms: i64,
    /// The person has said this one is fine. Kept rather than deleted, so it is not
    /// raised again the moment it is next read.
    pub accepted: bool,
}

impl StandingTrouble {
    /// How long it has been wrong, in whole days.
    #[must_use]
    pub const fn days_by(&self, now_ms: i64) -> i64 {
        (now_ms - self.since_ms) / 86_400_000
    }

    /// The problem statement, in the person's words — an observation with a duration and a
    /// question, which is what separates it from a status line.
    #[must_use]
    pub fn statement(&self, now_ms: i64) -> String {
        let days = self.days_by(now_ms);
        let how_long = match days {
            0 => "since earlier today".to_owned(),
            1 => "since yesterday".to_owned(),
            n => format!("for {n} days"),
        };
        format!("{} has not answered {how_long}", self.thing)
    }
}

/// One field a service's own setup form is asking for (ADR 0054).
///
/// Endora does not know what a calendar, a mail account or a doorbell needs. The service
/// does, and it will say — so the form is **rendered from what the service declares**, and
/// adding a kind of thing Endora has never heard of needs no code here at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetupField {
    /// The key to send the answer back under.
    pub name: String,
    /// `string`, `boolean`, or whatever else the service names its type.
    pub kind: String,
    /// Whether the form will not be accepted without it.
    pub required: bool,
    /// What the service suggests, when it suggests anything.
    pub default: Option<String>,
    /// Whether the answer is a secret, so the interface never echoes it back and Endora
    /// never writes it down.
    pub secret: bool,
}

impl SetupField {
    /// Whether a field name means a credential.
    ///
    /// A form Endora did not design can call a secret anything, so this is a heuristic —
    /// and it is one that fails **safe**: a field wrongly treated as secret is masked in
    /// the interface and still submitted correctly, while the reverse would put somebody's
    /// password on a screen.
    #[must_use]
    pub fn looks_secret(name: &str) -> bool {
        const CREDENTIAL_WORDS: &[&str] = &["password", "token", "secret", "api_key", "apikey"];
        let lowered = name.to_lowercase();
        CREDENTIAL_WORDS.iter().any(|w| lowered.contains(w))
    }
}

/// A setup form in progress, as the service described it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetupForm {
    /// The service's own handle for this attempt, sent back with the answers.
    pub id: String,
    /// Which step of the form this is, in the service's words.
    pub step: String,
    /// What it is asking for.
    pub fields: Vec<SetupField>,
}

/// Words a service uses to mean **"I cannot reach this"**, as opposed to a reading.
///
/// A heuristic, and named as one. There is no protocol-level way to ask "is this value a
/// real measurement or an admission of failure?" — services answer with whatever word they
/// use, and these are the words they use. The list is about English, not about any one
/// service, which is what keeps it out of a named adapter.
///
/// What makes a heuristic acceptable here is the blast radius: a wrong classification can
/// only ever produce **a question**, never an action. Getting it wrong costs one tap on
/// "it's fine"; getting the opposite wrong costs a device quietly staying broken.
///
/// **Only words that can mean nothing else.** The first list was wider — `unknown`, `none`,
/// `null`, `error` and an empty reading — and the first live reading refuted it: 28 things
/// were flagged against 7 real ones, and every single false positive was a *scene*, whose
/// state in Home Assistant is when it was last activated. `unknown` there means "not since
/// the last restart", which is the healthiest possible answer. Three days later the person
/// would have been asked about 28 working things, which is exactly the pile of chores this
/// was built to avoid, at scale.
///
/// So a word that means "hasn't happened yet" as often as it means "cannot be reached" is
/// not evidence of anything. `error` went too: a thing reporting an error **is** answering,
/// which is a different problem with a different remedy.
///
/// The cost is missing a device that only ever reports `unknown`. Accepted — those almost
/// always report `unavailable` as well, and a missed problem is recoverable while a butler
/// that cries wolf 28 times is not.
const NOT_A_READING: &[&str] = &["unavailable", "offline", "disconnected", "unreachable"];

/// Whether a state value is a service admitting it cannot see the thing.
#[must_use]
pub fn not_answering(state: &str) -> bool {
    NOT_A_READING.contains(&state.trim().to_lowercase().as_str())
}

/// How long something must be wrong before it is worth mentioning (ADR 0056).
///
/// Three days, chosen to survive the ordinary reasons a thing goes quiet without being
/// broken: a weekend away, a router reboot, a battery being swapped, a hub upgrade. The
/// cost of waiting is a few days' delay on a problem that has already lasted days; the cost
/// of not waiting is Endora interrupting about a device that was going to come back on its
/// own, which is the exact behaviour that makes a butler tiring.
pub const WORTH_SAYING_AFTER_DAYS: i64 = 3;

/// Which standing troubles are worth putting in front of the person right now.
///
/// Longest-standing first: the oldest is both the most likely to be genuinely finished with
/// and the one whose duration makes the strongest case.
#[must_use]
pub fn worth_raising(troubles: &[StandingTrouble], now_ms: i64) -> Vec<&StandingTrouble> {
    let mut out: Vec<&StandingTrouble> = troubles
        .iter()
        .filter(|t| !t.accepted)
        .filter(|t| t.days_by(now_ms) >= WORTH_SAYING_AFTER_DAYS)
        .collect();
    out.sort_by_key(|t| t.since_ms);
    // One device, one problem — whichever watcher saw it. Two sources watch the same
    // house (a connected server and the built-in reader), each keeping its own record,
    // and the person was handed every fault twice. Sorted oldest-first already, so the
    // record kept is the one whose duration makes the strongest case.
    let mut seen = std::collections::HashSet::new();
    out.retain(|t| seen.insert(t.thing.clone()));
    // …and one device is still one problem when it takes a dozen entities down with
    // it. A phone that stops reporting does not fail once; it fails per sensor, and
    // the person met eleven separate cards asking whether eleven halves of one phone
    // were still theirs. Sorted oldest-first, so the survivor is again the one whose
    // duration makes the case.
    //
    // Counted against what SURVIVED the de-duplication above, not the whole store:
    // an accepted or too-new sibling must not make a pile out of something that is
    // not one.
    let worth: Vec<(String, String)> = out
        .iter()
        .map(|t| (t.server.clone(), t.thing.clone()))
        .collect();
    let mut piled: std::collections::HashSet<String> = std::collections::HashSet::new();
    out.retain(|t| match a_pile_of(&worth, &t.server, &t.thing) {
        Some(device) => piled.insert(format!("{}::{device}", t.server)),
        None => true,
    });
    out
}

/// How many things a raised trouble stands for, counting itself — 1 when it
/// stands alone. What the card says out loud, so eleven things collapsing into
/// one line is visible rather than a silent truncation.
#[must_use]
pub fn how_many_things(
    troubles: &[StandingTrouble],
    now_ms: i64,
    raised: &StandingTrouble,
) -> usize {
    let worth: Vec<(String, String)> = troubles
        .iter()
        .filter(|t| !t.accepted)
        .filter(|t| t.days_by(now_ms) >= WORTH_SAYING_AFTER_DAYS)
        .map(|t| (t.server.clone(), t.thing.clone()))
        .collect();
    match a_pile_of(&worth, &raised.server, &raised.thing) {
        Some(device) => worth
            .iter()
            .filter(|(s, t)| s == &raised.server && device_of(t) == Some(device.as_str()))
            .count(),
        None => 1,
    }
}

/// How many of one device's things must be wrong before they stop being separate
/// problems.
///
/// Three, and the threshold is the whole safety of this: grouping only engages
/// where the alternative is already the pile [0056](../../docs/adr/0056-how-it-behaves-toward-you.md)
/// forbids. Two lights in one kitchen share a prefix and stay two cards, because
/// hiding a device somebody did not mean to hide is the failure worth avoiding —
/// and below three there is nothing to gain by risking it.
pub const A_PILE_IS: usize = 3;

/// The device an entity name belongs to, as far as a name can tell: the leading
/// token of the part after the domain. `sensor.bambam2_audio_output` → `bambam2`.
///
/// A heuristic, named as one. Services do not publish device membership in the
/// entity id, so this reads the convention every integration actually follows —
/// and it is only ever allowed to *group*, never to act, which bounds what being
/// wrong can cost.
#[must_use]
pub fn device_of(thing: &str) -> Option<&str> {
    let after_domain = thing.split_once('.').map_or(thing, |(_, rest)| rest);
    let head = after_domain.split('_').next()?;
    (head.len() >= 2 && head != after_domain).then_some(head)
}

/// The device group `thing` belongs to, when that group is a pile — `None` when
/// it stands alone, or when too few of its siblings are wrong to be worth
/// collapsing.
///
/// Shared by the raising and the answering on purpose: a card that claims to
/// cover eleven things must settle eleven things, and two readings of "what is
/// this card about?" would drift the first time either changed.
#[must_use]
pub fn a_pile_of(worth: &[(String, String)], server: &str, thing: &str) -> Option<String> {
    let device = device_of(thing)?;
    let siblings = worth
        .iter()
        .filter(|(s, t)| s == server && device_of(t) == Some(device))
        .count();
    (siblings >= A_PILE_IS).then(|| device.to_owned())
}

/// The person's **autonomy envelope** (ADR 0051): the deterministic boundary the
/// butler acts independently *within*. Widening it grants more independence; the
/// policy layer — never the model — still enforces the edges. This first slice has
/// two coarse levers; finer axes (spend vs. privacy, per-domain) come later.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AutonomyEnvelope {
    /// May the butler use read-only skills that **leave the device** (weather,
    /// news, a web page) on its own? Default yes.
    pub auto_external: bool,
    /// May it take **confirm-required** (consequential) actions on its own, rather
    /// than surfacing them for approval? Default no — the safe posture.
    pub auto_consequential: bool,
}

impl Default for AutonomyEnvelope {
    fn default() -> Self {
        // Preserves the established behaviour: read-only skills act on their own,
        // consequential ones ask (ADR 0051).
        Self {
            auto_external: true,
            auto_consequential: false,
        }
    }
}

/// How Endora reaches an [`McpServer`] (ADR 0054). Transport is an infrastructure
/// detail behind a port: `Stdio` (a local subprocess speaking MCP over its stdio)
/// ships first; `Http` (a networked MCP server over HTTP/SSE) is the same registry
/// shape with a different runtime. The domain only records *which*; it never speaks
/// the protocol.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpTransport {
    /// A local subprocess launched with `command` and `args`, spoken to over stdio.
    Stdio {
        /// The executable to launch, e.g. `"npx"`.
        command: String,
        /// Its arguments, e.g. `["-y", "@modelcontextprotocol/server-filesystem"]`.
        args: Vec<String>,
        /// Environment for the child process. Many servers take their credentials
        /// this way (e.g. `GITHUB_TOKEN`). Values are secrets: stored server-side and
        /// never returned to a client.
        env: std::collections::BTreeMap<String, String>,
    },
    /// A networked MCP server reached at `url` over HTTP/SSE.
    Http {
        /// The server's base URL.
        url: String,
        /// Optional bearer token sent as `Authorization: Bearer …` (e.g. a Home
        /// Assistant long-lived token). A secret: stored server-side, never returned.
        auth: String,
    },
}

/// A registered **MCP server** — a source of tools the butler's catalog can draw on
/// What the person calls something, and what that server actually calls it (ADR 0054).
///
/// Endora notices when a capability keeps failing or changing nothing on the same
/// target, and asks. This is the answer: *"kitchen main" means `Kitchen Main`*. It is
/// **confirmed** knowledge — supplied by the person, never inferred from a server's
/// text, which is the parsing ADR 0054 exists to avoid.
///
/// Keyed by **server**, not by tool: an entity's name belongs to the server, so one
/// answer covers every tool that server exposes rather than being re-asked per tool.
///
/// It reaches the model as **context**, the way a belief does. It is deliberately not a
/// substitution: the runner never rewrites the target a model asked for, because that
/// can act on the wrong thing and would hide the model's mistake from the eval battery
/// that exists to measure it (ADR 0053).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetAlias {
    /// The server whose vocabulary this is, e.g. `"home-assistant"`.
    pub server: String,
    /// What was asked for, in the words that kept failing.
    pub said: String,
    /// What that server actually calls it.
    pub means: String,
}

impl TargetAlias {
    /// Records what the person said the target really is. Trims all three.
    ///
    /// # Errors
    /// [`DomainError::EmptyField`] if any part is blank — an alias that names nothing
    /// on either side cannot ground anything.
    pub fn new(server: &str, said: &str, means: &str) -> Result<Self, DomainError> {
        Ok(Self {
            server: require_non_empty("alias.server", server)?,
            said: require_non_empty("alias.said", said)?,
            means: require_non_empty("alias.means", means)?,
        })
    }

    /// How it reads to the butler, as one grounded fact.
    #[must_use]
    pub fn as_context(&self) -> String {
        format!(
            "on {}, \"{}\" means \"{}\"",
            self.server, self.said, self.means
        )
    }
}

/// (ADR 0054). The registry row is plain data; the *act* of adding one is a gated
/// capability (deny-by-default), and every tool it exposes is still band-classified
/// before it can run (unknown ⇒ irreversible ⇒ blocked, ADR 0051). `name` is the
/// stable id and the namespacing prefix for this server's tools (`name.tool`), so it
/// must be non-empty.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpServer {
    /// Stable id and tool-namespacing prefix, e.g. `"filesystem"`.
    pub name: String,
    /// How Endora reaches it.
    pub transport: McpTransport,
    /// Whether the person has this server switched on. A disabled server contributes
    /// no tools to the catalog.
    pub enabled: bool,
    /// Auto-allow this server's tools on connect — **off by default**.
    ///
    /// It used to default **on**, which meant adding a server silently opened every tool
    /// it exposed. The doc here said opened tools still confirm each use, and that stopped
    /// being true once the person widened the envelope: opened plus
    /// `auto_consequential` is `Act`, with no confirmation.
    ///
    /// Live, to "Good morning": `HassBroadcast` — a tool that plays audio through the
    /// house — fired on a greeting. Nobody had chosen to open it. They had chosen to let
    /// consequential skills act on their own, which is a real decision about *skills they
    /// picked*, and this default quietly extended it to every tool a server happened to
    /// expose.
    ///
    /// [0054](../../docs/adr/0054-other-peoples-services.md) says MCP tools are
    /// deny-by-default and the person opens them individually. Now they are.
    pub trust_all: bool,
    /// The tool on this server that **reads its state** — the one Endora uses to check
    /// what an action actually did (ADR 0054). Empty means nobody has said.
    ///
    /// One answer settles two facts: the nominated tool's own result is an *observation*
    /// rather than a receipt, and every other tool on this server is verified through
    /// it. It replaces a hardcoded `Hass*` → `GetLiveContext` mapping that no other
    /// server could ever benefit from.
    ///
    /// Supplied by the person, so policy is never taking a third party's word for what
    /// its own tools do (ADR 0051). Endora may *suggest* a value; it never sets one.
    pub reader_tool: String,
}

impl McpServer {
    /// A local stdio server, enabled. Trims `name`/`command`; empty `args` are
    /// dropped so a blank field never becomes a spurious argument.
    ///
    /// # Errors
    /// [`DomainError::EmptyField`] if `name` or `command` is blank.
    pub fn stdio(
        name: &str,
        command: &str,
        args: impl IntoIterator<Item = String>,
    ) -> Result<Self, DomainError> {
        Self::stdio_with_env(name, command, args, std::collections::BTreeMap::new())
    }

    /// A local stdio server with an environment for the child process — how most
    /// servers take their credentials. Blank keys are dropped.
    ///
    /// # Errors
    /// [`DomainError::EmptyField`] if `name` or `command` is blank.
    pub fn stdio_with_env(
        name: &str,
        command: &str,
        args: impl IntoIterator<Item = String>,
        env: std::collections::BTreeMap<String, String>,
    ) -> Result<Self, DomainError> {
        let name = require_non_empty("mcp_server.name", name)?;
        let command = require_non_empty("mcp_server.command", command)?;
        let args = args
            .into_iter()
            .map(|a| a.trim().to_owned())
            .filter(|a| !a.is_empty())
            .collect();
        let env = env
            .into_iter()
            .map(|(k, v)| (k.trim().to_owned(), v))
            .filter(|(k, _)| !k.is_empty())
            .collect();
        Ok(Self {
            name,
            transport: McpTransport::Stdio { command, args, env },
            enabled: true,
            trust_all: false,
            reader_tool: String::new(),
        })
    }

    /// A networked HTTP/SSE server, enabled. Trims `name`/`url`.
    ///
    /// # Errors
    /// [`DomainError::EmptyField`] if `name` or `url` is blank.
    pub fn http(name: &str, url: &str) -> Result<Self, DomainError> {
        Self::http_with_auth(name, url, "")
    }

    /// A networked server with a bearer token (e.g. a Home Assistant long-lived
    /// token). An empty `auth` means no `Authorization` header is sent.
    ///
    /// # Errors
    /// [`DomainError::EmptyField`] if `name` or `url` is blank.
    pub fn http_with_auth(name: &str, url: &str, auth: &str) -> Result<Self, DomainError> {
        let name = require_non_empty("mcp_server.name", name)?;
        let url = require_non_empty("mcp_server.url", url)?;
        Ok(Self {
            name,
            transport: McpTransport::Http {
                url,
                auth: auth.trim().to_owned(),
            },
            enabled: true,
            trust_all: false,
            reader_tool: String::new(),
        })
    }

    /// Nominates the tool that reads this server's state (ADR 0054). Blank clears it.
    #[must_use]
    pub fn with_reader(mut self, tool: &str) -> Self {
        self.reader_tool = tool.trim().to_owned();
        self
    }
}

#[cfg(test)]
mod tests {
    use super::{McpServer, McpTransport};
    use endora_kernel::DomainError;

    #[test]
    fn a_phone_that_takes_its_sensors_down_with_it_is_one_card() {
        // Live, 2026-08-15: a phone stopped reporting and its eleven sensors each
        // became a separate "still yours?" card. One device failing is one
        // problem however many entities it publishes.
        use super::{StandingTrouble, WORTH_SAYING_AFTER_DAYS, how_many_things, worth_raising};
        const DAY: i64 = 86_400_000;
        let t = |thing: &str, since_ms: i64| StandingTrouble {
            server: "home-assistant".to_owned(),
            thing: thing.to_owned(),
            trouble: "unavailable".to_owned(),
            since_ms,
            accepted: false,
        };
        let troubles: Vec<StandingTrouble> = [
            "sensor.bambam2_audio_output",
            "sensor.bambam2_bssid",
            "sensor.bambam2_connection_type",
            "sensor.bambam2_kiosk_brightness",
            "sensor.bambam2_sim_1",
            "sensor.bambam2_storage",
        ]
        .iter()
        .enumerate()
        .map(|(n, thing)| t(thing, DAY + n as i64))
        .collect();
        let now = (WORTH_SAYING_AFTER_DAYS + 6) * DAY;
        let raised = worth_raising(&troubles, now);
        assert_eq!(raised.len(), 1, "one phone, one card: {raised:?}");
        // The oldest record survives — its duration makes the strongest case.
        assert_eq!(raised[0].thing, "sensor.bambam2_audio_output");
        // And the card can say what it stands for, rather than silently hiding ten.
        assert_eq!(how_many_things(&troubles, now, raised[0]), 6);
    }

    #[test]
    fn two_lights_in_one_room_are_still_two_problems() {
        // The guard on the heuristic: `light.kitchen_table` and
        // `light.kitchen_main` share a prefix and are genuinely different things.
        // Below the pile threshold nothing groups, because hiding a device
        // somebody did not mean to hide is the failure worth avoiding.
        use super::{StandingTrouble, WORTH_SAYING_AFTER_DAYS, worth_raising};
        const DAY: i64 = 86_400_000;
        let t = |thing: &str| StandingTrouble {
            server: "home-assistant".to_owned(),
            thing: thing.to_owned(),
            trouble: "unavailable".to_owned(),
            since_ms: DAY,
            accepted: false,
        };
        let troubles = vec![t("light.kitchen_table"), t("light.kitchen_main")];
        let now = (WORTH_SAYING_AFTER_DAYS + 2) * DAY;
        assert_eq!(worth_raising(&troubles, now).len(), 2);
    }

    #[test]
    fn a_name_with_no_device_part_groups_nothing() {
        use super::device_of;
        // Nothing to split on, or the whole name would be the device.
        assert_eq!(device_of("light.kitchen"), None);
        assert_eq!(device_of("weather.forecast_home"), Some("forecast"));
        assert_eq!(device_of("sensor.bambam2_audio_output"), Some("bambam2"));
        // A one-character head is noise, not a device.
        assert_eq!(device_of("sensor.a_b"), None);
    }

    #[test]
    fn one_dead_device_seen_by_two_watchers_is_one_problem() {
        // Live, 2026-08-06: every dead light was raised twice — once by the connected
        // server's watcher, once by the built-in reader's — and five real problems
        // became eleven, which the deploy smoke rightly called a pile of chores. The
        // fact is about the device, not about who saw it.
        use super::{StandingTrouble, WORTH_SAYING_AFTER_DAYS, worth_raising};
        const DAY: i64 = 86_400_000;
        let t = |server: &str, thing: &str, since_ms: i64| StandingTrouble {
            server: server.to_owned(),
            thing: thing.to_owned(),
            trouble: "unavailable".to_owned(),
            since_ms,
            accepted: false,
        };
        let troubles = vec![
            t("home-assistant", "light.guest_bedroom_right", DAY),
            t("skills", "light.guest_bedroom_right", 2 * DAY),
            t("skills", "light.outside_color", DAY),
        ];
        let raised = worth_raising(&troubles, (WORTH_SAYING_AFTER_DAYS + 3) * DAY);
        assert_eq!(raised.len(), 2, "one problem per device: {raised:?}");
        // The oldest record is the one kept: its duration makes the strongest case.
        let guest = raised
            .iter()
            .find(|t| t.thing == "light.guest_bedroom_right")
            .expect("raised");
        assert_eq!(guest.since_ms, DAY);
    }

    #[test]
    fn stdio_trims_and_drops_blank_args() {
        let s = McpServer::stdio(
            "  filesystem ",
            " npx ",
            ["-y".to_owned(), "  ".to_owned(), " server-fs ".to_owned()],
        )
        .unwrap();
        assert_eq!(s.name, "filesystem");
        assert!(s.enabled);
        assert_eq!(
            s.transport,
            McpTransport::Stdio {
                command: "npx".to_owned(),
                args: vec!["-y".to_owned(), "server-fs".to_owned()],
                env: std::collections::BTreeMap::new(),
            }
        );
    }

    #[test]
    fn stdio_env_carries_credentials_and_drops_blank_keys() {
        let env = [
            ("GITHUB_TOKEN".to_owned(), "sk-x".to_owned()),
            ("  ".to_owned(), "ignored".to_owned()),
        ]
        .into_iter()
        .collect();
        let s = McpServer::stdio_with_env("gh", "npx", ["server-github".to_owned()], env).unwrap();
        let McpTransport::Stdio { env, .. } = s.transport else {
            panic!("expected stdio")
        };
        assert_eq!(env.len(), 1);
        assert_eq!(env.get("GITHUB_TOKEN").map(String::as_str), Some("sk-x"));
    }

    #[test]
    fn http_auth_is_optional_and_trimmed() {
        let none = McpServer::http("cal", "https://cal.example").unwrap();
        assert_eq!(
            none.transport,
            McpTransport::Http {
                url: "https://cal.example".to_owned(),
                auth: String::new(),
            }
        );
        let tok = McpServer::http_with_auth("ha", "https://ha.local/mcp", "  abc123 ").unwrap();
        let McpTransport::Http { auth, .. } = tok.transport else {
            panic!("expected http")
        };
        assert_eq!(auth, "abc123");
    }

    #[test]
    fn http_requires_name_and_url() {
        assert_eq!(
            McpServer::http("", "https://x"),
            Err(DomainError::EmptyField {
                field: "mcp_server.name"
            })
        );
        assert_eq!(
            McpServer::http("cal", "   "),
            Err(DomainError::EmptyField {
                field: "mcp_server.url"
            })
        );
        assert_eq!(
            McpServer::http(" cal ", " https://cal.example ")
                .unwrap()
                .transport,
            McpTransport::Http {
                url: "https://cal.example".to_owned(),
                auth: String::new()
            }
        );
    }

    #[test]
    fn a_write_tells_an_addition_from_a_removal_by_what_was_there_before() {
        use super::{ConfigWrite, WriteKind};
        // Derived, not stored: a name already in the prior list can only have been taken
        // away. One table, one undo, and no flag that could disagree with the data.
        let added = ConfigWrite {
            id: 1,
            at_ms: 0,
            server: "home-assistant".to_owned(),
            target: "light.kitchen_table".to_owned(),
            added: "table".to_owned(),
            was: vec!["table light".to_owned()],
            undone: false,
            kind: WriteKind::Name,
        };
        assert!(!added.is_removal());
        assert!(
            added.describe().contains("now also answers to 'table'"),
            "{}",
            added.describe()
        );

        let removed = ConfigWrite {
            was: vec!["table light".to_owned(), "table".to_owned()],
            ..added
        };
        assert!(removed.is_removal());
        assert!(
            removed.describe().contains("no longer answers to 'table'"),
            "{}",
            removed.describe()
        );
    }
}

/// How long a reading must hold before it counts as a change.
///
/// **Wi-Fi presence flaps.** A phone sleeps its radio and vanishes for ten minutes while its
/// owner is on the sofa; an access point hands a device over and drops it for a beat. Written
/// straight into the record, that gives Endora *"you left"* fourteen times on a Tuesday — a
/// log worse than no log, because every later thing that reads it inherits the noise.
///
/// Five minutes matches what Home Assistant's own device trackers default to for the same
/// reason, and is short enough that a real departure is recorded while the person is still in
/// the car.
pub const DWELL_MS: i64 = 5 * 60 * 1_000;

/// One thing that really changed, and when.
///
/// The unit the transition log is made of. Deliberately about the **world**, not about
/// Endora: what it was, what it is, and the moment it moved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transition {
    /// Which thing, namespaced by the server that reported it.
    pub key: String,
    /// What it was before.
    pub from: String,
    /// What it became.
    pub to: String,
    /// When it changed — the first reading of the new state, not the pass that confirmed it.
    pub at_ms: i64,
}

/// What Endora has last seen of one thing, and what it is currently reading.
///
/// Two states rather than one, because "what it is" and "what it has just started saying" are
/// different questions and only the first is worth telling anybody.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Watched {
    /// Which thing, namespaced by the server that reported it.
    pub key: String,
    /// The last reading that held long enough to be believed.
    pub settled: String,
    /// What it has been saying since [`candidate_since_ms`](Self::candidate_since_ms). Equal
    /// to `settled` when nothing is in flight.
    pub candidate: String,
    /// When the current candidate first appeared.
    pub candidate_since_ms: i64,
}

/// What one reading amounted to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// Nothing worth writing down.
    Nothing,
    /// Something to remember about this thing, but no change to report: a first sighting, a
    /// new candidate starting its clock, or a flap that resolved itself.
    Noted(Watched),
    /// It really moved.
    Moved {
        /// What it was.
        from: String,
        /// What it is now.
        to: String,
        /// **When it actually changed**, not when Endora grew confident of it.
        at_ms: i64,
        /// The thing's new resting state.
        now: Watched,
    },
}

/// Normalises a reading, so casing and padding never look like movement.
fn as_read(state: &str) -> String {
    state.trim().to_lowercase()
}

/// Decides what a single reading means for one thing.
///
/// The whole transition log is this function; everything around it is storage. It is pure and
/// takes the clock as an argument, so every case below is a test rather than a wait.
///
/// A first sighting is deliberately **not** a change. Endora meeting an entity for the first
/// time would otherwise write a transition for every entity in the house the first time it
/// looks, and none of those happened.
#[must_use]
pub fn note_reading(prior: Option<&Watched>, key: &str, reading: &str, now_ms: i64) -> Change {
    let reading = as_read(reading);
    let Some(prior) = prior else {
        return Change::Noted(Watched {
            key: key.to_owned(),
            settled: reading.clone(),
            candidate: reading,
            candidate_since_ms: now_ms,
        });
    };

    // Back to where it started before settling: a flap that resolved itself, and nothing
    // happened. Dropping the candidate is what stops a fortnight of half-changes accumulating.
    if reading == prior.settled {
        if prior.candidate == prior.settled {
            return Change::Nothing;
        }
        return Change::Noted(Watched {
            candidate: prior.settled.clone(),
            candidate_since_ms: now_ms,
            ..prior.clone()
        });
    }

    // Something new, or something different again while the last one was still settling.
    if reading != prior.candidate {
        return Change::Noted(Watched {
            candidate: reading,
            candidate_since_ms: now_ms,
            ..prior.clone()
        });
    }

    // The same new reading as last time — has it held?
    let held_for = now_ms - prior.candidate_since_ms;
    if held_for < DWELL_MS {
        return Change::Nothing;
    }
    Change::Moved {
        from: prior.settled.clone(),
        to: reading.clone(),
        // The moment it changed, not the moment we believed it. A log that timestamps its own
        // confidence is a log about Endora rather than about the house.
        at_ms: prior.candidate_since_ms,
        now: Watched {
            settled: reading.clone(),
            candidate: reading,
            candidate_since_ms: prior.candidate_since_ms,
            ..prior.clone()
        },
    }
}

#[cfg(test)]
mod deciding_that_something_changed {
    //! The transition state machine, written before it existed.
    //!
    //! Wi-Fi presence flaps: a phone sleeps its radio and vanishes for ten minutes while its
    //! owner is on the sofa. Writing that into the record would give Endora "you left" fourteen
    //! times on a Tuesday, which is worse than having no record at all. So a reading has to
    //! *hold* before it counts as a change.

    use super::{Change, DWELL_MS, Watched, note_reading};

    fn watched(settled: &str, candidate: &str, since: i64) -> Watched {
        Watched {
            key: "house::light.kitchen".to_owned(),
            settled: settled.to_owned(),
            candidate: candidate.to_owned(),
            candidate_since_ms: since,
        }
    }

    #[test]
    fn the_first_sighting_of_a_thing_is_not_a_change() {
        // Endora has just met this entity. Recording "nothing → on" would put a transition in
        // the log for every entity in the house the first time it looks, and none of them
        // happened.
        let change = note_reading(None, "house::light.kitchen", "on", 1_000);
        let Change::Noted(now) = change else {
            panic!("expected a first sighting to be noted, got {change:?}");
        };
        assert_eq!(now.settled, "on");
        assert_eq!(now.candidate, "on");
        assert_eq!(now.candidate_since_ms, 1_000);
    }

    #[test]
    fn a_reading_that_has_not_moved_says_nothing() {
        let prior = watched("on", "on", 1_000);
        assert!(matches!(
            note_reading(Some(&prior), &prior.key, "on", 9_999),
            Change::Nothing
        ));
    }

    #[test]
    fn a_new_reading_starts_settling_rather_than_counting() {
        let prior = watched("on", "on", 1_000);
        let Change::Noted(now) = note_reading(Some(&prior), &prior.key, "off", 2_000) else {
            panic!("expected it to start settling");
        };
        assert_eq!(now.settled, "on", "nothing has changed yet");
        assert_eq!(now.candidate, "off");
        assert_eq!(now.candidate_since_ms, 2_000);
    }

    #[test]
    fn a_reading_that_holds_long_enough_becomes_a_transition() {
        let prior = watched("on", "off", 2_000);
        let Change::Moved {
            from,
            to,
            at_ms,
            now,
        } = note_reading(Some(&prior), &prior.key, "off", 2_000 + DWELL_MS)
        else {
            panic!("expected a transition once it held");
        };
        assert_eq!(from, "on");
        assert_eq!(to, "off");
        assert_eq!(
            at_ms, 2_000,
            "timestamped when it actually changed, not when we grew confident"
        );
        assert_eq!(now.settled, "off");
    }

    #[test]
    fn a_flap_that_goes_back_before_settling_never_happened() {
        // The whole reason for the dwell. The phone dropped off Wi-Fi and came back.
        let prior = watched("home", "not_home", 2_000);
        let change = note_reading(Some(&prior), &prior.key, "home", 2_000 + DWELL_MS / 2);
        let Change::Noted(now) = change else {
            panic!("expected the flap to be forgotten, got {change:?}");
        };
        assert_eq!(now.settled, "home");
        assert_eq!(now.candidate, "home", "the candidate is abandoned");
    }

    #[test]
    fn a_reading_that_changes_again_while_settling_restarts_the_clock() {
        // on → off → dim, all inside the dwell. Nothing has held, so nothing has happened.
        let prior = watched("on", "off", 2_000);
        let Change::Noted(now) = note_reading(Some(&prior), &prior.key, "dim", 3_000) else {
            panic!("expected the clock to restart");
        };
        assert_eq!(now.settled, "on");
        assert_eq!(now.candidate, "dim");
        assert_eq!(now.candidate_since_ms, 3_000);
    }

    #[test]
    fn one_tick_short_of_the_dwell_is_not_yet_a_change() {
        let prior = watched("on", "off", 2_000);
        assert!(matches!(
            note_reading(Some(&prior), &prior.key, "off", 2_000 + DWELL_MS - 1),
            Change::Nothing
        ));
    }

    #[test]
    fn a_clock_that_goes_backwards_commits_nothing() {
        let prior = watched("on", "off", 10_000);
        assert!(matches!(
            note_reading(Some(&prior), &prior.key, "off", 1),
            Change::Nothing
        ));
    }

    #[test]
    fn case_and_padding_do_not_make_a_change() {
        // Services are inconsistent about this, and a transition log that records "On" → "on"
        // would fill up with events nobody had.
        let prior = watched("on", "on", 1_000);
        assert!(matches!(
            note_reading(Some(&prior), &prior.key, "  On ", 9_999),
            Change::Nothing
        ));
    }

    fn latlon() -> Vec<super::RecipeInput> {
        vec![
            super::RecipeInput {
                name: "lat".to_owned(),
                kind: super::RecipeInputKind::Number,
            },
            super::RecipeInput {
                name: "lon".to_owned(),
                kind: super::RecipeInputKind::Number,
            },
        ]
    }

    #[test]
    fn the_worked_example_from_the_adr_builds() {
        let r = super::Recipe::new(
            "air_quality",
            "Today's air quality where you are.",
            latlon(),
            "https://air-quality-api.open-meteo.com/v1/air-quality?latitude={lat}&longitude={lon}&current=us_aqi",
            "The air quality index is {current.us_aqi} right now.",
        )
        .unwrap();
        assert_eq!(r.capability_id(), "recipe.air_quality");
    }

    #[test]
    fn an_id_with_unsafe_characters_is_refused() {
        for bad in [
            "Air Quality",
            "air.quality",
            "air/quality",
            "1air",
            "-air",
            "",
        ] {
            assert!(
                super::Recipe::new(bad, "d", vec![], "https://x.test/", "s").is_err(),
                "{bad:?} should have been refused"
            );
        }
    }

    #[test]
    fn a_get_template_must_be_http_or_https() {
        assert!(
            super::Recipe::new("x", "d", vec![], "ftp://x.test/", "s").is_err(),
            "a scheme other than http(s) is not a request a recipe may make"
        );
        assert!(
            super::Recipe::new("x", "d", vec![], "file:///etc/passwd", "s").is_err(),
            "a local file is not a request a recipe may make"
        );
    }

    #[test]
    fn a_get_slot_naming_an_undeclared_input_is_refused_at_authoring_time() {
        // Caught here, not discovered as a runtime 400 the first time someone
        // enables it — the whole point of validating at authoring time.
        let err = super::Recipe::new(
            "x",
            "d",
            latlon(),
            "https://x.test/?latitude={lat}&longitude={lon}&zip={zip}",
            "s",
        )
        .unwrap_err();
        assert!(matches!(err, endora_kernel::DomainError::Malformed { .. }));
    }

    #[test]
    fn an_undeclared_slot_in_say_is_left_for_the_interpreter() {
        // `say` reads from the live response, which the domain layer has never
        // seen — only `get`'s slots are checkable against declared inputs here.
        assert!(
            super::Recipe::new(
                "x",
                "d",
                latlon(),
                "https://x.test/?latitude={lat}&longitude={lon}",
                "{deeply.nested.value}",
            )
            .is_ok()
        );
    }

    #[test]
    fn placeholders_in_reads_names_in_order() {
        assert_eq!(super::placeholders_in("{a}-{b}/{c}"), vec!["a", "b", "c"]);
        assert_eq!(super::placeholders_in("no slots here"), Vec::<&str>::new());
        // An unclosed brace contributes nothing rather than panicking.
        assert_eq!(super::placeholders_in("{a}-{unclosed"), vec!["a"]);
    }

    #[test]
    fn recipe_input_kind_round_trips_by_name() {
        assert_eq!(
            super::RecipeInputKind::from_name("string"),
            Some(super::RecipeInputKind::Text)
        );
        assert_eq!(
            super::RecipeInputKind::from_name("number"),
            Some(super::RecipeInputKind::Number)
        );
        assert_eq!(super::RecipeInputKind::from_name("bool"), None);
        assert_eq!(super::RecipeInputKind::Text.as_str(), "string");
    }
}
