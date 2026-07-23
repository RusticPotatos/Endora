//! Capabilities domain model — the autonomy envelope and the MCP server registry.

use endora_kernel::DomainError;
use endora_kernel::error::require_non_empty;

/// The person's **autonomy envelope** (ADR 0022): the deterministic boundary the
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
        // consequential ones ask (ADR 0010).
        Self {
            auto_external: true,
            auto_consequential: false,
        }
    }
}

/// How Endora reaches an [`McpServer`] (ADR 0021). Transport is an infrastructure
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
    },
    /// A networked MCP server reached at `url` over HTTP/SSE.
    Http {
        /// The server's base URL.
        url: String,
    },
}

/// A registered **MCP server** — a source of tools the butler's catalog can draw on
/// (ADR 0021). The registry row is plain data; the *act* of adding one is a gated
/// capability (deny-by-default), and every tool it exposes is still band-classified
/// before it can run (unknown ⇒ irreversible ⇒ blocked, ADR 0024). `name` is the
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
        let name = require_non_empty("mcp_server.name", name)?;
        let command = require_non_empty("mcp_server.command", command)?;
        let args = args
            .into_iter()
            .map(|a| a.trim().to_owned())
            .filter(|a| !a.is_empty())
            .collect();
        Ok(Self {
            name,
            transport: McpTransport::Stdio { command, args },
            enabled: true,
        })
    }

    /// A networked HTTP/SSE server, enabled. Trims `name`/`url`.
    ///
    /// # Errors
    /// [`DomainError::EmptyField`] if `name` or `url` is blank.
    pub fn http(name: &str, url: &str) -> Result<Self, DomainError> {
        let name = require_non_empty("mcp_server.name", name)?;
        let url = require_non_empty("mcp_server.url", url)?;
        Ok(Self {
            name,
            transport: McpTransport::Http { url },
            enabled: true,
        })
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
            }
        );
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
                url: "https://cal.example".to_owned()
            }
        );
    }
}
