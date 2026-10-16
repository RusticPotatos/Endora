//! Bluetooth audio, on request only. The device is invisible and unpairable unless a pairing
//! window is open, so it does not show up on a work network. Audio itself is handled by the
//! system's audio server: a phone that connects plays through the current output, and a
//! paired speaker can be chosen as the output.
use crate::Result;
use std::{
    io::{Read, Write},
    process::{Command, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};

/// The plain name shown while a pairing window is open.
pub const ALIAS: &str = "Pi Speaker";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Device {
    pub mac: String,
    pub name: String,
}

pub fn is_mac(value: &str) -> bool {
    value.len() == 17
        && value.split(':').count() == 6
        && value
            .split(':')
            .all(|g| g.len() == 2 && g.bytes().all(|b| b.is_ascii_hexdigit()))
}

pub fn alias_ok(value: &str) -> bool {
    let value = value.trim();
    !value.is_empty()
        && value.len() <= 32
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.'))
}

/// Named devices from `bluetoothctl devices` output. Entries whose "name" is just the address
/// (unnamed advertisers), malformed addresses and duplicates are dropped.
pub fn parse_devices(text: &str) -> Vec<Device> {
    let mut out: Vec<Device> = Vec::new();
    for line in text.lines() {
        let Some(rest) = line.strip_prefix("Device ") else {
            continue;
        };
        let Some((mac, name)) = rest.split_once(' ') else {
            continue;
        };
        let name = name.trim();
        if !is_mac(mac)
            || name.is_empty()
            || name.replace('-', ":").eq_ignore_ascii_case(mac)
            || out.iter().any(|d| d.mac.eq_ignore_ascii_case(mac))
        {
            continue;
        }
        out.push(Device {
            mac: mac.to_owned(),
            name: name.chars().filter(|c| !c.is_control()).take(40).collect(),
        });
    }
    out
}

/// A 1-based menu number picks a device.
pub fn pick<'a>(devices: &'a [Device], input: &str) -> Option<&'a Device> {
    let n: usize = input.trim().parse().ok()?;
    devices.get(n.checked_sub(1)?)
}

fn ctl(args: &[&str]) -> Option<String> {
    let out = Command::new("bluetoothctl").args(args).output().ok()?;
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Controller addresses from `bluetoothctl list`, sorted so the order (and so each adapter's
/// name) does not change when a USB adapter is plugged in and takes over as "default".
pub fn parse_controllers(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in text.lines() {
        let Some(rest) = line.strip_prefix("Controller ") else {
            continue;
        };
        let mac = rest.split_whitespace().next().unwrap_or("");
        if is_mac(mac) && !out.iter().any(|m| m.eq_ignore_ascii_case(mac)) {
            out.push(mac.to_owned());
        }
    }
    out.sort();
    out
}

/// How an adapter is attached.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bus {
    Usb,
    Builtin,
    Other,
}

/// Each adapter's address and attachment from `hciconfig -a` output.
pub fn parse_buses(text: &str) -> Vec<(String, Bus)> {
    let mut bus = None;
    let mut out = Vec::new();
    for line in text.lines() {
        if !line.starts_with(char::is_whitespace) && line.contains(':') {
            bus = None;
        }
        if let Some((_, rest)) = line.split_once("Bus: ") {
            bus = Some(match rest.split_whitespace().next() {
                Some("USB") => Bus::Usb,
                Some("UART" | "SDIO" | "RS232") => Bus::Builtin,
                _ => Bus::Other,
            });
        }
        if let Some(rest) = line.trim().strip_prefix("BD Address: ") {
            let mac = rest.split_whitespace().next().unwrap_or("");
            if let (true, Some(bus)) = (is_mac(mac), bus.take()) {
                out.push((mac.to_owned(), bus));
            }
        }
    }
    out
}

/// The name each adapter shows while pairing: "Pi Speaker USB" for a dongle, "Pi Speaker
/// Built-in" for the board's own chip, numbered when two are alike, so the right one can be
/// picked on the phone or Mac.
pub fn name_adapters(macs: &[String], buses: &[(String, Bus)]) -> Vec<(String, String)> {
    let mut bases: Vec<&str> = Vec::new();
    macs.iter()
        .map(|mac| {
            let base = match buses.iter().find(|(m, _)| m.eq_ignore_ascii_case(mac)) {
                Some((_, Bus::Usb)) => "Pi Speaker USB",
                Some((_, Bus::Builtin)) => "Pi Speaker Built-in",
                _ => ALIAS,
            };
            let seen = bases.iter().filter(|b| **b == base).count();
            bases.push(base);
            let alias = if seen == 0 {
                base.to_owned()
            } else {
                format!("{base} {}", seen + 1)
            };
            (mac.clone(), alias)
        })
        .collect()
}

/// Every adapter with the name it shows while pairing.
pub fn adapters() -> Vec<(String, String)> {
    let buses = Command::new("hciconfig")
        .arg("-a")
        .output()
        .map(|o| parse_buses(&String::from_utf8_lossy(&o.stdout)))
        .unwrap_or_default();
    name_adapters(&controllers(), &buses)
}

/// A plain numbered name, used when the attachment of an adapter is unknown.
pub fn adapter_alias(index: usize) -> String {
    if index == 0 {
        ALIAS.to_owned()
    } else {
        format!("{ALIAS} {}", index + 1)
    }
}

/// Removes terminal colour codes, carriage returns and the `[bluetooth]#` prompt that
/// `bluetoothctl` prints in front of answers when its commands are piped in.
pub fn clean_output(raw: &str) -> String {
    let mut plain = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\x1b' => {
                if chars.peek() == Some(&'[') {
                    chars.next();
                    for n in chars.by_ref() {
                        if n.is_ascii_alphabetic() {
                            break;
                        }
                    }
                }
            }
            '\r' => plain.push('\n'),
            c => plain.push(c),
        }
    }
    plain
        .lines()
        .map(|line| match (line.starts_with('['), line.find("]# ")) {
            (true, Some(at)) => line[at + 3..].trim_start(),
            _ => line,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn controllers() -> Vec<String> {
    parse_controllers(&ctl(&["list"]).unwrap_or_default())
}

/// How long a piped `bluetoothctl` session is given.
enum Wait<'a> {
    /// A quick query: done once the answers stop arriving.
    Quiet,
    /// A slow action: done when one of the phrases shows up, or after this many seconds.
    Until(&'a [&'a str], u64),
    /// Always this many seconds (scanning).
    Secs(u64),
}

/// Runs commands in one `bluetoothctl` session, first selecting `controller` when given (the
/// one-shot form can only reach the default adapter), and returns its cleaned output.
fn session(controller: Option<&str>, commands: &[&str], wait: Wait) -> String {
    let Ok(mut child) = Command::new("bluetoothctl")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    else {
        return String::new();
    };
    let (Some(mut input), Some(mut stdout)) = (child.stdin.take(), child.stdout.take()) else {
        let _ = child.kill();
        return String::new();
    };
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        while let Ok(n) = stdout.read(&mut buf) {
            if n == 0
                || tx
                    .send(String::from_utf8_lossy(&buf[..n]).into_owned())
                    .is_err()
            {
                break;
            }
        }
    });
    let mut script = String::new();
    if let Some(mac) = controller {
        script.push_str(&format!("select {mac}\n"));
    }
    for command in commands {
        script.push_str(command);
        script.push('\n');
    }
    let _ = input.write_all(script.as_bytes());
    let (start, mut last, mut raw) = (Instant::now(), Instant::now(), String::new());
    loop {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(chunk) => {
                raw.push_str(&chunk);
                last = Instant::now();
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
        let done = match &wait {
            Wait::Quiet => {
                last.elapsed() >= Duration::from_millis(500)
                    || start.elapsed() >= Duration::from_secs(8)
            }
            Wait::Until(phrases, seconds) => {
                start.elapsed() >= Duration::from_secs(*seconds) || {
                    let text = clean_output(&raw);
                    phrases.iter().any(|p| text.contains(p))
                }
            }
            Wait::Secs(seconds) => start.elapsed() >= Duration::from_secs(*seconds),
        };
        if done {
            break;
        }
    }
    let _ = input.write_all(b"quit\n");
    drop(input);
    let quit_by = Instant::now() + Duration::from_secs(2);
    while child.try_wait().ok().flatten().is_none() {
        if Instant::now() >= quit_by {
            let _ = child.kill();
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    clean_output(&raw)
}

/// The same commands on every adapter, in order.
fn on_each(commands: &[&str], wait: impl Fn() -> Wait<'static>) -> Vec<String> {
    controllers()
        .iter()
        .map(|c| session(Some(c), commands, wait()))
        .collect()
}

/// Devices from every adapter, each listed once.
fn devices_matching(filter: &str) -> Vec<Device> {
    let command = format!("devices {filter}");
    let mut out: Vec<Device> = Vec::new();
    for text in on_each(&[command.trim()], || Wait::Quiet) {
        for d in parse_devices(&text) {
            if !out.iter().any(|o| o.mac.eq_ignore_ascii_case(&d.mac)) {
                out.push(d);
            }
        }
    }
    out
}

/// The adapter that knows this device (the one it was found or paired through).
fn owner(mac: &str) -> Option<String> {
    controllers().into_iter().find(|c| {
        parse_devices(&session(Some(c), &["devices"], Wait::Quiet))
            .iter()
            .any(|d| d.mac.eq_ignore_ascii_case(mac))
    })
}

fn on_owner(device: &Device, commands: &[&str], wait: Wait) -> Result<String> {
    if !is_mac(&device.mac) {
        return Err("Invalid Bluetooth address".into());
    }
    let adapter = owner(&device.mac).ok_or("No Bluetooth adapter knows that device")?;
    Ok(session(Some(&adapter), commands, wait))
}

fn is_connected(device: &Device) -> bool {
    on_owner(device, &[&format!("info {}", device.mac)], Wait::Quiet)
        .is_ok_and(|t| t.contains("Connected: yes"))
}

pub fn paired() -> Vec<Device> {
    devices_matching("Paired")
}

/// Devices in range, found by every adapter scanning at the same time for `seconds`.
pub fn scan(seconds: u32) -> Vec<Device> {
    let adapters = controllers();
    let handles: Vec<_> = adapters
        .into_iter()
        .map(|c| {
            std::thread::spawn(move || {
                let _ = session(Some(&c), &["scan on"], Wait::Secs(u64::from(seconds)));
                parse_devices(&session(Some(&c), &["devices"], Wait::Quiet))
            })
        })
        .collect();
    let mut out: Vec<Device> = Vec::new();
    for handle in handles {
        for d in handle.join().unwrap_or_default() {
            if !out.iter().any(|o| o.mac.eq_ignore_ascii_case(&d.mac)) {
                out.push(d);
            }
        }
    }
    out
}

pub fn pair_and_connect(device: &Device) -> Result<()> {
    let mac = device.mac.as_str();
    on_owner(
        device,
        &[&format!("pair {mac}")],
        Wait::Until(
            &["Pairing successful", "Failed to pair", "AlreadyExists"],
            30,
        ),
    )?;
    on_owner(device, &[&format!("trust {mac}")], Wait::Quiet)?;
    on_owner(
        device,
        &[&format!("connect {mac}")],
        Wait::Until(&["Connection successful", "Failed to connect"], 20),
    )?;
    if is_connected(device) {
        Ok(())
    } else {
        Err("Could not connect; put the speaker in pairing mode and try again".into())
    }
}

pub fn forget(device: &Device) -> Result<()> {
    on_owner(device, &[&format!("remove {}", device.mac)], Wait::Quiet).map(|_| ())
}

pub fn connect(device: &Device) -> Result<()> {
    on_owner(
        device,
        &[&format!("connect {}", device.mac)],
        Wait::Until(&["Connection successful", "Failed to connect"], 20),
    )?;
    if is_connected(device) {
        Ok(())
    } else {
        Err("Not connected; is it on and in range?".into())
    }
}

/// `bluetoothctl` registers its own confirm-a-code agent on start, so it is removed first
/// (`agent off`); otherwise the phone or Mac shows a code that nothing here can confirm.
/// One command (or a few) sent to `bluetoothctl`, after waiting `wait_ms`.
pub struct Step {
    pub wait_ms: u64,
    pub text: String,
}

/// The commands that open a pairing window. `bluetoothctl` registers its own confirm-a-code agent
/// a moment after it starts, so the silent "no input, no output" agent can only replace it after a
/// pause; sent immediately, the replacement fails and the phone or Mac shows a code that nothing
/// here can confirm. The discoverable timeout is a safety net if this program dies.
pub fn pairing_steps(seconds: u64) -> Vec<Step> {
    pairing_steps_for(seconds, &[])
}

/// The same script for several adapters: every adapter is powered on first, one agent is set up
/// (it is shared by all adapters), then each adapter gets its own name and is made visible, so the
/// other device lists "Speaker", "Speaker 2" and so on. An empty list uses the default adapter.
pub fn pairing_steps_for(seconds: u64, adapters: &[(String, String)]) -> Vec<Step> {
    let step = |wait_ms: u64, text: &str| Step {
        wait_ms,
        text: text.to_owned(),
    };
    let mut power = String::new();
    let mut open = String::from("default-agent\n");
    if adapters.is_empty() {
        power.push_str("power on\n");
        open.push_str(&format!(
            "system-alias \"{ALIAS}\"\ndiscoverable-timeout {}\npairable on\ndiscoverable on\n",
            seconds + 30
        ));
    }
    for (mac, alias) in adapters {
        power.push_str(&format!("select {mac}\npower on\n"));
        open.push_str(&format!(
            "select {mac}\nsystem-alias \"{alias}\"\ndiscoverable-timeout {}\npairable on\ndiscoverable on\n",
            seconds + 30
        ));
    }
    vec![
        step(0, &power),
        step(3000, "agent off\n"),
        step(1500, "agent NoInputNoOutput\n"),
        step(1500, &open),
    ]
}

/// Watches for a new device during a pairing window: trust it at once (an untrusted device's
/// first audio connection needs an answer nothing can give), and close a few seconds later so
/// that first connection can finish.
#[derive(Default)]
pub struct Settle {
    seen: bool,
    ticks: u32,
}

impl Settle {
    /// One tick per second. Returns (trust the paired devices now, close the window now).
    pub fn tick(&mut self, paired_now: usize, paired_before: usize) -> (bool, bool) {
        if !self.seen {
            if paired_now > paired_before {
                self.seen = true;
                return (true, false);
            }
            return (false, false);
        }
        self.ticks += 1;
        (false, self.ticks >= 6)
    }
}

pub fn trust_paired() {
    for adapter in controllers() {
        let known = parse_devices(&session(Some(&adapter), &["devices Paired"], Wait::Quiet));
        if known.is_empty() {
            continue;
        }
        let commands: Vec<String> = known.iter().map(|d| format!("trust {}", d.mac)).collect();
        let commands: Vec<&str> = commands.iter().map(String::as_str).collect();
        session(Some(&adapter), &commands, Wait::Quiet);
    }
}

/// What to try when the controller does not become discoverable. Escalating: try again, restart
/// the Bluetooth service, then reload the driver, which power-cycles the chip itself (the chip on
/// this board sometimes stops answering commands; a service restart does not fix that).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Recovery {
    Retry,
    RestartService,
    ReloadDriver,
}

pub fn recovery_ladder(failures: u32) -> Option<Recovery> {
    match failures {
        0 => Some(Recovery::Retry),
        1 => Some(Recovery::RestartService),
        2 => Some(Recovery::ReloadDriver),
        _ => None,
    }
}

impl Recovery {
    pub fn message(self) -> &'static str {
        match self {
            Self::Retry => "Bluetooth did not open; trying again...",
            Self::RestartService => {
                "Still not open; restarting Bluetooth (connected devices will drop)..."
            }
            Self::ReloadDriver => {
                "Bluetooth chip is not responding; resetting it (connected devices will drop)..."
            }
        }
    }

    fn run(self) {
        match self {
            Self::Retry => {}
            Self::RestartService => {
                let _ = reset_service();
            }
            Self::ReloadDriver => {
                let _ = reload_driver();
            }
        }
    }
}

/// True when at least one adapter is currently visible to other devices.
pub fn is_discoverable() -> bool {
    on_each(&["show"], || Wait::Quiet)
        .iter()
        .any(|t| t.contains("Discoverable: yes"))
}

/// Unloads and reloads the Bluetooth driver, which power-cycles the controller chip. Pairings and
/// the device address are kept. Needs passwordless sudo.
pub fn reload_driver() -> Result<()> {
    let sudo = |args: &[&str]| {
        Command::new("sudo")
            .arg("-n")
            .args(args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    };
    let pause = |s| std::thread::sleep(Duration::from_secs(s));
    let _ = sudo(&["systemctl", "stop", "bluetooth", "hciuart"]);
    pause(1);
    if !sudo(&["modprobe", "-r", "hci_uart"]) {
        return Err("Cannot reload the Bluetooth driver (needs sudo)".into());
    }
    let _ = sudo(&["modprobe", "-r", "btbcm"]);
    pause(3);
    let _ = sudo(&["modprobe", "hci_uart"]);
    pause(6);
    let _ = sudo(&["systemctl", "start", "hciuart"]);
    pause(3);
    let _ = sudo(&["systemctl", "start", "bluetooth"]);
    pause(5);
    set_power(true);
    let _ = Command::new("systemctl")
        .args(["--user", "restart", "wireplumber"])
        .status();
    Ok(())
}

/// Opens a pairing window: visible and pairable (no PIN; audio devices only need confirmation on
/// their side) for up to `seconds`, then closed again. If the controller does not become visible
/// it is reset in escalating steps (`note` is told what is happening). `tick` gets the seconds
/// left and returns true to end early. Devices that paired are trusted so they reconnect by
/// themselves later, without another window.
pub fn pairing_window(
    seconds: u64,
    mut note: impl FnMut(&str),
    mut tick: impl FnMut(u64) -> bool,
) -> Result<Vec<Device>> {
    let mut failures = 0;
    let (mut child, mut input) = loop {
        let adapters = adapters();
        if adapters.is_empty() && failures == 0 {
            note("No Bluetooth adapter found; trying to recover it...");
        }
        let mut child = Command::new("bluetoothctl")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| "Bluetooth unavailable")?;
        let mut input = child.stdin.take().ok_or("Bluetooth unavailable")?;
        for step in pairing_steps_for(seconds, &adapters) {
            std::thread::sleep(Duration::from_millis(step.wait_ms));
            let _ = input.write_all(step.text.as_bytes());
        }
        std::thread::sleep(Duration::from_millis(1500));
        if is_discoverable() {
            break (child, input);
        }
        let _ = input.write_all(b"quit\n");
        drop(input);
        let _ = child.wait();
        let Some(step) = recovery_ladder(failures) else {
            return Err("Bluetooth is not responding; restart the device".into());
        };
        note(step.message());
        step.run();
        failures += 1;
    };
    if failures > 0 {
        note("Bluetooth recovered; pairing window open");
    }
    let end = Instant::now() + Duration::from_secs(seconds);
    loop {
        let left = end.saturating_duration_since(Instant::now()).as_secs();
        if left == 0 || tick(left) {
            break;
        }
        std::thread::sleep(Duration::from_secs(1));
    }
    let mut closing = String::new();
    for mac in controllers() {
        closing.push_str(&format!("select {mac}\ndiscoverable off\npairable off\n"));
    }
    closing.push_str("quit\n");
    let _ = input.write_all(closing.as_bytes());
    drop(input);
    let _ = child.wait();
    trust_paired();
    Ok(paired())
}

/// Whether the radio should be on: only on the home network, never with no Wi-Fi or on another
/// network, to save battery. `None` means no opinion yet (home is not known and Wi-Fi is up).
pub fn bluetooth_wanted(home: crate::vpn::HomeMatch, net: &str) -> Option<bool> {
    use crate::vpn::HomeMatch::{Away, Home, Unknown};
    let online = !matches!(net, "No Wi-Fi" | "NetworkManager unavailable");
    match home {
        Home => Some(online),
        Away => Some(false),
        Unknown => (!online).then_some(false),
    }
}

/// Whether to switch adapters back on: at home and online, with at least one adapter off (a
/// power glitch resets the USB adapter), and not because it was turned off by hand.
pub fn needs_power_on(wanted: Option<bool>, turned_off_by_hand: bool, powered: &[bool]) -> bool {
    wanted == Some(true) && !turned_off_by_hand && powered.contains(&false)
}

/// Connected devices that have no sound device in the audio server. A phone or Mac that
/// connects on its own often leaves out the audio link until this side opens it.
pub fn missing_audio_cards(connected: &[Device], cards: &str) -> Vec<Device> {
    let cards = cards.to_ascii_lowercase();
    connected
        .iter()
        .filter(|d| {
            let card = format!("bluez_card.{}", d.mac.replace(':', "_")).to_ascii_lowercase();
            !cards.contains(&card)
        })
        .cloned()
        .collect()
}

/// Opens the audio link to connected devices that offer sound but have none yet. A device that
/// sends no sound (a keyboard, say) is left alone.
pub fn restore_audio() {
    let cards = Command::new("pactl")
        .args(["list", "short", "cards"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    for device in missing_audio_cards(&connected(), &cards) {
        let offers_sound = on_owner(&device, &[&format!("info {}", device.mac)], Wait::Quiet)
            .is_ok_and(|t| t.contains("Audio Source"));
        if offers_sound {
            let _ = connect(&device);
        }
    }
}

/// The power state of each adapter.
pub fn adapter_power() -> Vec<bool> {
    on_each(&["show"], || Wait::Quiet)
        .iter()
        .map(|t| t.contains("Powered: yes"))
        .collect()
}

/// Applies the radio decision only after it has held for three checks in a row (about 30
/// seconds), and only when it differs from what was last applied, so a brief Wi-Fi flicker never
/// drops a connected headset. A manual change stays until the network situation really changes.
#[derive(Default)]
pub struct Policy {
    applied: Option<bool>,
    pending: Option<bool>,
    ticks: u32,
}

impl Policy {
    pub fn observe(&mut self, want: Option<bool>) -> Option<bool> {
        let Some(want) = want else {
            self.pending = None;
            self.ticks = 0;
            return None;
        };
        if self.applied == Some(want) {
            self.pending = None;
            self.ticks = 0;
            return None;
        }
        if self.pending == Some(want) {
            self.ticks += 1;
        } else {
            self.pending = Some(want);
            self.ticks = 1;
        }
        if self.ticks >= 3 {
            self.applied = Some(want);
            self.pending = None;
            self.ticks = 0;
            Some(want)
        } else {
            None
        }
    }
}

/// Turns every adapter on or off.
pub fn set_power(on: bool) {
    let command = if on { "power on" } else { "power off" };
    on_each(&[command], || Wait::Quiet);
}

/// Devices connected right now through any adapter (empty when the radios are off).
pub fn connected() -> Vec<Device> {
    devices_matching("Connected")
}

/// True when any adapter is on.
pub fn powered() -> bool {
    on_each(&["show"], || Wait::Quiet)
        .iter()
        .any(|t| t.contains("Powered: yes"))
}

/// Devices in `now` that were not in `before` (addresses compared without regard to case).
pub fn new_devices(before: &[Device], now: &[Device]) -> Vec<Device> {
    now.iter()
        .filter(|d| !before.iter().any(|b| b.mac.eq_ignore_ascii_case(&d.mac)))
        .cloned()
        .collect()
}

/// Forgets one paired device.
pub fn forget_one(device: &Device) -> Result<()> {
    forget(device)
}

pub fn disconnect(device: &Device) -> Result<()> {
    on_owner(
        device,
        &[&format!("disconnect {}", device.mac)],
        Wait::Quiet,
    )
    .map(|_| ())
}

/// Forgets every paired device and returns how many there were. Only for an explicit "forget all".
pub fn forget_all() -> usize {
    let mut count = 0;
    for adapter in controllers() {
        let known = parse_devices(&session(Some(&adapter), &["devices"], Wait::Quiet));
        if known.is_empty() {
            continue;
        }
        let commands: Vec<String> = known
            .iter()
            .flat_map(|d| [format!("disconnect {}", d.mac), format!("remove {}", d.mac)])
            .collect();
        let commands: Vec<&str> = commands.iter().map(String::as_str).collect();
        session(Some(&adapter), &commands, Wait::Quiet);
        count += known.len();
    }
    count
}

/// Restarts the Bluetooth service and the audio session that talks to it. Pairings survive.
pub fn reset_service() -> Result<()> {
    let status = Command::new("sudo")
        .args(["-n", "systemctl", "restart", "bluetooth"])
        .status()
        .map_err(|_| "Cannot restart Bluetooth")?;
    if !status.success() {
        return Err("Cannot restart Bluetooth (needs sudo)".into());
    }
    std::thread::sleep(Duration::from_secs(3));
    set_power(true);
    let _ = Command::new("systemctl")
        .args(["--user", "restart", "wireplumber"])
        .status();
    Ok(())
}
