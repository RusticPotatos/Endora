use crate::Result;
use std::process::Command;
#[derive(Clone)]
pub struct Wifi {
    pub ssid: String,
    pub connected: bool,
    pub signal: u8,
    pub secured: bool,
}
pub fn parse_wifi(text: &str) -> Vec<Wifi> {
    let mut rows: Vec<Wifi> = Vec::new();
    for line in text.lines() {
        let mut columns = vec![String::new()];
        let mut escaped = false;
        for ch in line.chars() {
            if escaped {
                columns.last_mut().unwrap().push(ch);
                escaped = false;
                continue;
            }
            if ch == '\\' {
                escaped = true;
                continue;
            }
            if ch == ':' {
                columns.push(String::new());
                continue;
            }
            columns.last_mut().unwrap().push(ch);
        }
        if columns.len() != 4 || columns[1].is_empty() || rows.iter().any(|r| r.ssid == columns[1])
        {
            continue;
        }
        rows.push(Wifi {
            connected: columns[0] == "*",
            ssid: columns[1].clone(),
            signal: columns[2].parse().unwrap_or(0),
            secured: !matches!(columns[3].as_str(), "--" | ""),
        });
    }
    rows
}
pub fn scan() -> Result<Vec<Wifi>> {
    let result = Command::new("nmcli")
        .args([
            "-t",
            "-e",
            "yes",
            "-f",
            "IN-USE,SSID,SIGNAL,SECURITY",
            "device",
            "wifi",
            "list",
            "--rescan",
            "yes",
        ])
        .output()
        .map_err(|_| "NetworkManager unavailable")?;
    if !result.status.success() {
        return Err("Wi-Fi scan failed".into());
    }
    Ok(parse_wifi(&String::from_utf8_lossy(&result.stdout)))
}
pub fn state() -> &'static str {
    let output = Command::new("nmcli")
        .args(["-t", "-f", "STATE,CONNECTIVITY", "general"])
        .output();
    let Ok(output) = output else {
        return "NetworkManager unavailable";
    };
    let text = String::from_utf8_lossy(&output.stdout);
    if text.contains(":portal") {
        return "Portal login needed";
    }
    if text.contains(":full") {
        return "Internet available";
    }
    if text.starts_with("connecting") {
        return "Joining";
    }
    if text.starts_with("connected") {
        return "Connected; checking internet";
    }
    "No Wi-Fi"
}

/// True once each time the network turns into a captive portal, so the login is offered but
/// not repeated while it is open or after it was declined. Seeing the network become definitely
/// not a portal (online, or no Wi-Fi) re-arms it for the next portal; brief in-between states
/// change nothing.
pub fn portal_offer(offered: &mut bool, state: &str) -> bool {
    match state {
        "Portal login needed" if !*offered => {
            *offered = true;
            true
        }
        "Internet available" | "No Wi-Fi" => {
            *offered = false;
            false
        }
        _ => false,
    }
}

/// The Wi-Fi device's addresses, for the Network settings screen.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NetDetails {
    pub network: String,
    pub signal: Option<u8>,
    pub ip: String,
    pub mac: String,
    pub gateway: String,
    pub dns: Vec<String>,
}

/// Reads `nmcli -t dev show` output (`KEY:value` lines).
pub fn parse_device_show(text: &str) -> NetDetails {
    let mut d = NetDetails::default();
    for line in text.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value: String = value
            .trim()
            .chars()
            .filter(|c| !c.is_control())
            .take(64)
            .collect();
        if value.is_empty() || value == "--" {
            continue;
        }
        match key {
            "GENERAL.HWADDR" => d.mac = value,
            "GENERAL.CONNECTION" => d.network = value,
            "IP4.GATEWAY" => d.gateway = value,
            k if k.starts_with("IP4.ADDRESS") && d.ip.is_empty() => d.ip = value,
            k if k.starts_with("IP4.DNS") => d.dns.push(value),
            _ => {}
        }
    }
    d
}

/// The details as screen lines, with the VPN state and, if a manual choice is holding it, how
/// many minutes are left before automatic switching resumes.
pub fn net_lines(d: &NetDetails, vpn: &str, hold_minutes: Option<u64>) -> Vec<String> {
    let mut lines = Vec::new();
    if d.network.is_empty() && d.ip.is_empty() {
        lines.push("Wi-Fi   Not connected".to_owned());
    } else {
        let signal = d
            .signal
            .map_or(String::new(), |s| format!(" (signal {s}%)"));
        lines.push(format!("Wi-Fi   {}{signal}", d.network));
        lines.push(format!("IP      {}", d.ip));
        lines.push(format!("MAC     {}", d.mac));
        lines.push(format!("Gateway {}", d.gateway));
        lines.push(format!("DNS     {}", d.dns.join(", ")));
    }
    lines.push(format!("VPN     {vpn}"));
    if let Some(minutes) = hold_minutes {
        lines.push(format!(
            "Auto VPN paused {minutes} min (your manual choice)"
        ));
    }
    lines
        .into_iter()
        .map(|l| l.chars().take(54).collect())
        .collect()
}

/// The Wi-Fi device's details right now.
pub fn details() -> NetDetails {
    let run = |args: &[&str]| {
        Command::new("nmcli")
            .args(args)
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
            .unwrap_or_default()
    };
    let mut d = parse_device_show(&run(&[
        "-t",
        "-f",
        "GENERAL.HWADDR,GENERAL.CONNECTION,IP4.ADDRESS,IP4.GATEWAY,IP4.DNS",
        "dev",
        "show",
        "wlan0",
    ]));
    d.signal = run(&[
        "-t",
        "-f",
        "IN-USE,SIGNAL",
        "dev",
        "wifi",
        "list",
        "--rescan",
        "no",
    ])
    .lines()
    .find(|l| l.starts_with('*'))
    .and_then(|l| l.rsplit(':').next())
    .and_then(|s| s.trim().parse().ok());
    d
}
