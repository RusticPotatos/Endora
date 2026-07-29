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
const NOT_A_READING: &[&str] = &[
    "unavailable",
    "unknown",
    "offline",
    "disconnected",
    "unreachable",
    "none",
    "null",
    "error",
];

/// Whether a state value is a service admitting it cannot see the thing.
#[must_use]
pub fn not_answering(state: &str) -> bool {
    let value = state.trim().to_lowercase();
    value.is_empty() || NOT_A_READING.contains(&value.as_str())
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
    out
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
    /// Auto-allow this server's tools on connect. When set, every tool it exposes is
    /// opened for use without per-tool clicking — but opened MCP tools are still
    /// `Block`→`Confirm`, so the butler asks before each use (ADR 0051). The enabling
    /// is done in code from this stored flag; it is never driven by model output. Off
    /// keeps the stricter deny-by-default, where each tool stays blocked until allowed.
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
            trust_all: true,
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
            trust_all: true,
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
