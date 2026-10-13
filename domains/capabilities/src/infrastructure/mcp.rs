use super::*;

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
pub(super) struct McpConnection {
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
pub(super) fn compact_input_hint(schema: &serde_json::Value) -> Option<String> {
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
pub(super) const MOST_RESOURCES_READ_PER_SERVER: usize = 200;

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
                        // A server Endora has no relationship with is answering with
                        // somebody else's words (ADR 0064, using 0058's own line): a search
                        // server is a stranger, the house is not. The overlay that knows
                        // which servers are the house sets this; here it is the safe
                        // default, because a tool nobody has placed is a stranger.
                        wants_place: false,
                        third_party: true,
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
        let args = coerce_args_to_schema(
            input_json,
            tool.input_schema.as_ref(),
            self.is_state_reader(id),
        );
        conn.transport.call(&tool.name, &args)
    }
}
