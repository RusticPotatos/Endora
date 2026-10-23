//! The explicit team tool-contract boundary, independent of MCP transport dates.

/// Validate the supported canonical team route without printing configured URLs.
/// # Errors
/// An old or unsupported endpoint, before any worker request.
pub fn canonical_endpoint(endpoint: &str) -> Result<&str, &'static str> {
    const UPGRADE: &str = "team protocol v2 is required: configure /v2/team/mcp or /team-gateway/v2/mcp without URL credentials, query or fragment";
    let (scheme, rest) = endpoint.split_once("://").ok_or(UPGRADE)?;
    let (authority, path) = rest.split_once('/').ok_or(UPGRADE)?;
    if !matches!(scheme, "http" | "https")
        || authority.is_empty()
        || authority.contains(['@', '?', '#'])
        || endpoint
            .chars()
            .any(|c| c.is_whitespace() || c.is_control())
        || !matches!(path, "v2/team/mcp" | "team-gateway/v2/mcp")
    {
        return Err(UPGRADE);
    }
    Ok(endpoint)
}

#[cfg(test)]
mod tests {
    use super::canonical_endpoint;

    #[test]
    fn canonical_team_routes_accept_direct_and_gateway_configuration() {
        for endpoint in [
            "http://hq:8790/v2/team/mcp",
            "https://example.com/team-gateway/v2/mcp",
        ] {
            assert_eq!(canonical_endpoint(endpoint), Ok(endpoint));
        }
    }

    #[test]
    fn old_or_ambiguous_routes_fail_without_echoing_credentials() {
        for endpoint in [
            "http://hq:8790/v1/team/mcp",
            "http://node:8788/team-gateway/mcp",
            "http://hq:8790/v2/team/mcp/",
            "http://hq:8790/v2/team/mcp?key=fixture-secret",
            "http://hq:8790/v2/team/mcp#fixture-secret",
            "http://user:fixture-secret@hq:8790/v2/team/mcp",
            "file://hq/v2/team/mcp",
            "http:///v2/team/mcp",
            "not-a-url",
            "http://hq\n/v2/team/mcp",
        ] {
            let error = canonical_endpoint(endpoint).unwrap_err();
            assert!(error.contains("v2"));
            assert!(!error.contains("fixture-secret"));
        }
    }
}
