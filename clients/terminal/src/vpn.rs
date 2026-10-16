//! Cloudflare WARP as the away-from-home tunnel: its state, when to connect, and the top-bar icons.
//!
//! The rule is deliberately one-sided. The tunnel routes private address ranges, so at home it
//! can cut local access (including SSH). It is therefore never connected on the home network,
//! and is dropped if it is found up there. Home is recognised by a fingerprint of the Wi-Fi
//! gateway's hardware address, learned the first time the node answers directly.
use std::{net::ToSocketAddrs, process::Command, time::Duration};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Vpn {
    /// No `warp-cli`, or its answer was not understood.
    #[default]
    Unavailable,
    /// Installed but not enrolled in the organization.
    Unregistered,
    Disconnected,
    Connecting,
    Connected,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HomeMatch {
    Home,
    Away,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Connect,
    Disconnect,
    Nothing,
}

pub struct Facts {
    pub vpn: Vpn,
    pub home: HomeMatch,
    pub internet: bool,
    pub node_reachable: bool,
    /// Consecutive checks (about ten seconds apart) in which the node did not answer.
    pub unreachable_ticks: u32,
    /// The tunnel was connected or disconnected by hand recently: leave it as it is.
    pub manual_hold: bool,
}

pub fn parse_warp_status(text: &str) -> Vpn {
    let first = text.lines().next().unwrap_or("");
    let Some(state) = first.strip_prefix("Status update:") else {
        return Vpn::Unavailable;
    };
    match state.trim() {
        "Connected" => Vpn::Connected,
        "Disconnected" => Vpn::Disconnected,
        s if s.starts_with("Connecting") => Vpn::Connecting,
        "Unable" if text.contains("Registration Missing") => Vpn::Unregistered,
        _ => Vpn::Unavailable,
    }
}

pub fn decide(f: &Facts) -> Action {
    if f.manual_hold {
        return Action::Nothing;
    }
    match (f.vpn, f.home) {
        (Vpn::Unavailable | Vpn::Unregistered, _) => Action::Nothing,
        (Vpn::Connected | Vpn::Connecting, HomeMatch::Home) => Action::Disconnect,
        (Vpn::Disconnected, HomeMatch::Away | HomeMatch::Unknown)
            if f.internet && !f.node_reachable && f.unreachable_ticks >= 3 =>
        {
            Action::Connect
        }
        _ => Action::Nothing,
    }
}

/// A stable, non-reversible-looking token for a gateway hardware address.
pub fn fingerprint(mac: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in mac.trim().to_ascii_lowercase().bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

pub fn home_match(stored: Option<&str>, current: Option<&str>) -> HomeMatch {
    match (stored, current) {
        (Some(a), Some(b)) if a == b => HomeMatch::Home,
        (Some(_), Some(_)) => HomeMatch::Away,
        _ => HomeMatch::Unknown,
    }
}

/// Remember the current network as home only when nothing is stored yet, the tunnel is down
/// and the node answers directly, so the first direct success defines home.
pub fn should_learn_home(
    vpn: Vpn,
    node_reachable: bool,
    stored: Option<&str>,
    current: Option<&str>,
) -> bool {
    vpn == Vpn::Disconnected && node_reachable && stored.is_none() && current.is_some()
}

/// Top-bar text for the connection: the tunnel (only when up or coming up), then Wi-Fi.
pub fn status_icons(net: &str, vpn: Vpn, bluetooth: bool) -> String {
    let wifi = match net {
        "Internet available" | "Home connected" => "Wi-Fi",
        "Portal login needed" | "Connected; checking internet" => "Wi-Fi!",
        "Joining" => "Wi-Fi...",
        _ => "No Wi-Fi",
    };
    let tunnel = match vpn {
        Vpn::Connected => format!("VPN  {wifi}"),
        Vpn::Connecting => format!("VPN..  {wifi}"),
        _ => wifi.to_owned(),
    };
    if bluetooth {
        format!("BT  {tunnel}")
    } else {
        tunnel
    }
}

fn warp(args: &[&str]) -> Option<String> {
    let out = Command::new("warp-cli")
        .arg("--accept-tos")
        .args(args)
        .output()
        .ok()?;
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

pub fn status() -> Vpn {
    warp(&["status"]).map_or(Vpn::Unavailable, |t| parse_warp_status(&t))
}

pub fn connect() {
    let _ = warp(&["connect"]);
}

pub fn disconnect() {
    let _ = warp(&["disconnect"]);
}

/// What Cloudflare's trace page says about this connection.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Trace {
    /// "on" when traffic goes through WARP.
    pub warp: Option<String>,
    /// The public address the internet sees.
    pub ip: Option<String>,
}

pub fn parse_trace(text: &str) -> Trace {
    let field = |name: &str| {
        text.lines()
            .find_map(|l| l.strip_prefix(&format!("{name}=")))
            .map(|v| {
                v.trim()
                    .chars()
                    .filter(|c| !c.is_control())
                    .take(64)
                    .collect()
            })
    };
    Trace {
        warp: field("warp"),
        ip: field("ip"),
    }
}

/// Asks Cloudflare (which runs the WARP tunnel) whether this connection goes through it.
pub fn trace() -> Option<Trace> {
    let out = Command::new("curl")
        .args(["-s", "-m", "6", "https://www.cloudflare.com/cdn-cgi/trace"])
        .output()
        .ok()?;
    Some(parse_trace(&String::from_utf8_lossy(&out.stdout))).filter(|t| t.warp.is_some())
}

/// A fingerprint of the default gateway's hardware address, if it can be read.
pub fn gateway_id() -> Option<String> {
    let route = Command::new("ip")
        .args(["-4", "route", "show", "default"])
        .output()
        .ok()?;
    let route = String::from_utf8_lossy(&route.stdout).into_owned();
    let gateway = route
        .split_whitespace()
        .skip_while(|w| *w != "via")
        .nth(1)?
        .to_owned();
    let neigh = Command::new("ip")
        .args(["neigh", "show", &gateway])
        .output()
        .ok()?;
    let neigh = String::from_utf8_lossy(&neigh.stdout).into_owned();
    let mac = neigh
        .split_whitespace()
        .skip_while(|w| *w != "lladdr")
        .nth(1)?;
    Some(fingerprint(mac))
}

/// Whether the node's host and port accept a connection right now, without the tunnel's help
/// being assumed: it only says the address answers.
pub fn node_reachable(url: &str) -> bool {
    let rest = url.split("://").nth(1).unwrap_or(url);
    let authority = rest.split('/').next().unwrap_or(rest);
    let Some(addr) = authority.to_socket_addrs().ok().and_then(|mut a| a.next()) else {
        return false;
    };
    std::net::TcpStream::connect_timeout(&addr, Duration::from_secs(3)).is_ok()
}

/// Automatic connecting can be turned off by creating `~/.config/endora-terminal/no-auto-vpn`.
pub fn auto_enabled() -> bool {
    crate::storage::Paths::config()
        .map(|p| !p.root.join("no-auto-vpn").exists())
        .unwrap_or(true)
}

pub fn load_home() -> Option<String> {
    let bytes = crate::storage::Paths::state()
        .ok()?
        .read("home-network")
        .ok()?;
    let text = String::from_utf8(bytes).ok()?;
    let text = text.trim();
    (text.len() == 16 && text.bytes().all(|b| b.is_ascii_hexdigit())).then(|| text.to_owned())
}

pub fn save_home(id: &str) {
    if let Ok(paths) = crate::storage::Paths::state() {
        let _ = paths.write("home-network", id.as_bytes());
    }
}
