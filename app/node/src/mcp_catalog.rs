//! A small, curated catalog of well-known MCP servers (ADR 0054).
//!
//! These are **starting points, not fixed recipes**: an entry prefills the "Add a
//! server" form (transport, command, args, the env keys it wants) and everything
//! stays editable before it is registered. That matters because upstream package
//! names drift — if a launch command changes, the person can correct it in the form
//! rather than waiting on a release here. Each entry carries a docs link to check.
//!
//! The catalog is deliberately node-level configuration data, not domain: it says
//! nothing about policy. Anything installed from it is still deny-by-default — its
//! tools are visible but blocked until the person allows each one (ADR 0051).

use serde_json::{Value, json};

/// Where a value the person supplies is applied when registering the server.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// Appended to the launch arguments (e.g. a folder to expose).
    Arg,
    /// Set in the child process's environment (e.g. an API token).
    Env,
    /// The HTTP endpoint URL.
    Url,
    /// The HTTP bearer token.
    Auth,
}

impl Target {
    fn name(self) -> &'static str {
        match self {
            Self::Arg => "arg",
            Self::Env => "env",
            Self::Url => "url",
            Self::Auth => "auth",
        }
    }
}

/// One value the person must supply to install an entry.
pub struct Field {
    /// For `Env` this is the variable name; otherwise a short identifier.
    pub key: &'static str,
    pub label: &'static str,
    pub placeholder: &'static str,
    pub secret: bool,
    pub target: Target,
}

/// A catalog entry: enough to prefill the add form.
pub struct Entry {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub category: &'static str,
    /// `"stdio"` or `"http"`.
    pub transport: &'static str,
    /// Launch command for a stdio server.
    pub command: &'static str,
    /// Fixed launch arguments; the person's `Arg` fields are appended after these.
    pub args: &'static [&'static str],
    pub fields: &'static [Field],
    pub docs: &'static str,
}

/// The curated entries. Kept short and general on purpose — the live registry search
/// covers breadth, this covers "the ones most people want, ready to edit".
pub const ENTRIES: &[Entry] = &[
    Entry {
        id: "filesystem",
        name: "Filesystem",
        description: "Read and write files under a folder you choose.",
        category: "files",
        transport: "stdio",
        command: "npx",
        args: &["-y", "@modelcontextprotocol/server-filesystem"],
        fields: &[Field {
            key: "path",
            label: "Folder to expose",
            placeholder: "/data",
            secret: false,
            target: Target::Arg,
        }],
        docs: "https://github.com/modelcontextprotocol/servers",
    },
    Entry {
        id: "memory",
        name: "Memory",
        description: "A persistent knowledge graph the butler can store notes in.",
        category: "knowledge",
        transport: "stdio",
        command: "npx",
        args: &["-y", "@modelcontextprotocol/server-memory"],
        fields: &[],
        docs: "https://github.com/modelcontextprotocol/servers",
    },
    Entry {
        id: "everything",
        name: "Everything (test server)",
        description: "The reference server — useful to check the MCP plumbing works.",
        category: "testing",
        transport: "stdio",
        command: "npx",
        args: &["-y", "@modelcontextprotocol/server-everything"],
        fields: &[],
        docs: "https://github.com/modelcontextprotocol/servers",
    },
    Entry {
        id: "sequential-thinking",
        name: "Sequential thinking",
        description: "A structured step-by-step reasoning aid.",
        category: "reasoning",
        transport: "stdio",
        command: "npx",
        args: &["-y", "@modelcontextprotocol/server-sequentialthinking"],
        fields: &[],
        docs: "https://github.com/modelcontextprotocol/servers",
    },
    Entry {
        id: "github",
        name: "GitHub",
        description: "Repositories, issues and pull requests.",
        category: "dev",
        transport: "stdio",
        command: "npx",
        args: &["-y", "@modelcontextprotocol/server-github"],
        fields: &[Field {
            key: "GITHUB_PERSONAL_ACCESS_TOKEN",
            label: "GitHub token",
            placeholder: "ghp_…",
            secret: true,
            target: Target::Env,
        }],
        docs: "https://github.com/modelcontextprotocol/servers",
    },
    Entry {
        id: "git",
        name: "Git",
        description: "Inspect and search a local git repository.",
        category: "dev",
        transport: "stdio",
        command: "uvx",
        args: &["mcp-server-git", "--repository"],
        fields: &[Field {
            key: "repo",
            label: "Repository path",
            placeholder: "/data/repo",
            secret: false,
            target: Target::Arg,
        }],
        docs: "https://github.com/modelcontextprotocol/servers",
    },
    // Web search, which is the one nobody had and everybody wanted. Endora can reach a
    // house, a calendar and a mailbox and still cannot answer "when do they open" — and
    // searching the registry for it is where this entry earns its place rather than being a
    // convenience. The registry sorts by recency, so a stranger's fork of a Brave server was
    // the first result and Brave's own was fourth, on a search whose whole purpose is to
    // hand somebody's API key to whatever comes back. Curated entries are checked and they
    // sort first; that is the difference between the two lists.
    //
    // Package and variable are the ones Brave publishes under `io.github.brave/`, read from
    // the registry rather than remembered.
    Entry {
        id: "brave-search",
        name: "Brave Search",
        description: "Search the web — an independent index, not a wrapper around Google \
                      or Bing. Needs a free API key from Brave.",
        category: "search web",
        transport: "stdio",
        command: "npx",
        args: &["-y", "@brave/brave-search-mcp-server"],
        fields: &[Field {
            key: "BRAVE_API_KEY",
            label: "Brave Search API key",
            placeholder: "BSA…",
            secret: true,
            target: Target::Env,
        }],
        docs: "https://brave.com/search/api/",
    },
    Entry {
        id: "fetch",
        name: "Fetch",
        description: "Fetch a web page and hand back readable text.",
        category: "web",
        transport: "stdio",
        command: "uvx",
        args: &["mcp-server-fetch"],
        fields: &[],
        docs: "https://github.com/modelcontextprotocol/servers",
    },
    Entry {
        id: "home-assistant",
        name: "Home Assistant",
        description: "Your Home Assistant, via its Model Context Protocol Server \
                      integration. Enable that integration in HA, then paste its MCP \
                      URL and a long-lived access token.",
        category: "home",
        transport: "http",
        command: "",
        args: &[],
        fields: &[
            Field {
                key: "url",
                label: "MCP endpoint URL",
                placeholder: "http://homeassistant.local:8123/mcp_server/sse",
                secret: false,
                target: Target::Url,
            },
            Field {
                key: "auth",
                label: "Long-lived access token",
                placeholder: "stored securely, never shown",
                secret: true,
                target: Target::Auth,
            },
        ],
        docs: "https://www.home-assistant.io/integrations/mcp_server/",
    },
    Entry {
        id: "mcp-gateway",
        name: "MCP Gateway (Docker)",
        description: "One endpoint aggregating servers you run as containers — the \
                      lean option: Endora connects, Docker runs them.",
        category: "gateway",
        transport: "http",
        command: "",
        args: &[],
        fields: &[Field {
            key: "url",
            label: "Gateway URL",
            placeholder: "http://mcp-gateway:8080/",
            secret: false,
            target: Target::Url,
        }],
        docs: "https://docs.docker.com/ai/mcp-catalog-and-toolkit/",
    },
];

/// Renders one entry as JSON for the console.
fn entry_json(e: &Entry) -> Value {
    json!({
        "id": e.id,
        "name": e.name,
        "description": e.description,
        "category": e.category,
        "transport": e.transport,
        "command": e.command,
        "args": e.args,
        "docs": e.docs,
        "source": "curated",
        "fields": e.fields.iter().map(|f| json!({
            "key": f.key,
            "label": f.label,
            "placeholder": f.placeholder,
            "secret": f.secret,
            "target": f.target.name(),
        })).collect::<Vec<_>>(),
    })
}

/// The curated entries matching `q` (case-insensitive over name/description/
/// category/id). An empty query returns everything.
pub fn search(q: &str) -> Vec<Value> {
    let needle = q.trim().to_lowercase();
    ENTRIES
        .iter()
        .filter(|e| {
            needle.is_empty()
                || e.id.to_lowercase().contains(&needle)
                || e.name.to_lowercase().contains(&needle)
                || e.description.to_lowercase().contains(&needle)
                || e.category.to_lowercase().contains(&needle)
        })
        .map(entry_json)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{ENTRIES, search};

    #[test]
    fn every_entry_is_installable_as_written() {
        for e in ENTRIES {
            assert!(
                !e.id.is_empty() && !e.name.is_empty(),
                "entry needs id+name"
            );
            match e.transport {
                // A stdio entry must say what to run.
                "stdio" => assert!(!e.command.is_empty(), "{} needs a command", e.id),
                // An http entry can't hardcode a URL — it must ask for one.
                "http" => assert!(
                    e.fields.iter().any(|f| f.key == "url"),
                    "{} must ask for a URL",
                    e.id
                ),
                other => panic!("{} has an unknown transport '{other}'", e.id),
            }
        }
    }

    #[test]
    fn search_filters_and_empty_returns_all() {
        assert_eq!(search("").len(), ENTRIES.len());
        let home = search("home assistant");
        assert!(home.is_empty() || home[0]["id"] == "home-assistant");
        // Matches on category too.
        assert!(!search("dev").is_empty());
        assert!(search("zzzznope").is_empty());
    }

    /// Whatever somebody calls it, searching for the web finds a way to search it.
    ///
    /// This is a trust check wearing a search check's clothes. Without a curated answer the
    /// query falls straight through to the registry, which sorts by recency and put a
    /// stranger's fork above Brave's own — on the one search where the next step is typing
    /// an API key into whatever came back first.
    #[test]
    fn searching_for_the_web_finds_a_way_to_search_it() {
        for asked in ["brave", "search", "web"] {
            let found = search(asked);
            assert!(
                found.iter().any(|e| e["id"] == "brave-search"),
                "'{asked}' found no way to search the web"
            );
        }
    }

    /// A curated entry that wants a credential has to ask for it by name.
    ///
    /// Registry entries often declare nothing, and then the only way in is the Advanced
    /// KEY=value box — a plain-text field for a secret, where a mistyped key fails silently
    /// and the server just returns nothing. A curated entry exists to be better than that.
    #[test]
    fn a_curated_entry_asks_for_its_credential_by_name() {
        let brave = ENTRIES.iter().find(|e| e.id == "brave-search").unwrap();
        let key = brave.fields.iter().find(|f| f.secret).unwrap();
        assert_eq!(key.key, "BRAVE_API_KEY");
        assert!(
            !brave.docs.is_empty(),
            "a key somebody has to go and get needs a link to where they get it"
        );
    }
}
