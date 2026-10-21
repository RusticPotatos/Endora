use super::*;

/// Keeps the tool servers usable for the team's bridge (ADR 0108): every second minute an
/// enabled server that came up with no tools is tried again. Observed after a reboot: every
/// container started at once, Endora reached the Home Assistant server before it was listening,
/// got an empty catalogue, and never retried. `connect_mcp` runs once at startup, which is
/// exactly wrong on a machine where everything boots together. What the butler's heartbeat also
/// did (the daily brief, watching the house for the chat, withdrawing tools the chat's outcomes
/// said never worked) is retired with the chat (ADR 0136).
pub fn spawn_tool_server_upkeep(state: AppState) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(120));
        // The first tick is at once: the servers have just been connected.
        ticker.tick().await;
        loop {
            ticker.tick().await;
            reconnect_empty_mcp_servers(&state);
        }
    });
}
