//! A left/right sound test for whatever the device is playing through (the USB dongle or a
//! Bluetooth speaker), so a one-sided sound can be traced to the source, the cable or the output.
use crate::Result;
use std::{
    io::{BufRead, BufReader, Write},
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::Duration,
};

pub const RATE: u32 = 22_050;

/// The test in order: what to call it on screen, then whether the left and right ears play.
pub const STEPS: [(&str, bool, bool); 3] = [
    ("left ear", true, false),
    ("right ear", false, true),
    ("both ears", true, true),
];

/// Raw 16-bit stereo audio of a 440 Hz tone for `ms` milliseconds, in the chosen ears only. It
/// fades in and out so there is no click.
pub fn tone(left: bool, right: bool, ms: u32) -> Vec<u8> {
    let frames = (RATE as u64 * u64::from(ms) / 1000) as usize;
    let fade = (RATE as usize / 100).max(1);
    let mut out = Vec::with_capacity(frames * 4);
    for i in 0..frames {
        let edge = i.min(frames - 1 - i).min(fade) as f32 / fade as f32;
        let wave = (2.0 * std::f32::consts::PI * 440.0 * i as f32 / RATE as f32).sin();
        let value = (wave * edge * 8000.0) as i16;
        for on in [left, right] {
            out.extend_from_slice(&(if on { value } else { 0 }).to_le_bytes());
        }
    }
    out
}

/// Plays one tone through the audio server's current output and waits for it to finish.
pub fn play(left: bool, right: bool) -> Result<()> {
    let mut child = Command::new("pacat")
        .args(["--playback", "--raw", "--format=s16le", "--channels=2"])
        .arg(format!("--rate={RATE}"))
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "Cannot play sound (pacat is missing)")?;
    let mut input = child.stdin.take().ok_or("Cannot play sound")?;
    let written = input.write_all(&tone(left, right, 1200)).is_ok();
    drop(input);
    let finished = child.wait().is_ok_and(|s| s.success());
    if written && finished {
        Ok(())
    } else {
        Err("The sound could not be played; is an output connected?".into())
    }
}

/// What the audio side looks like right now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Health {
    /// The USB sound card is registered with the kernel.
    pub card: bool,
    /// The USB device itself is on the bus (it can be there without a sound card).
    pub device: bool,
    /// The audio server has an output for it.
    pub sink: bool,
}

/// How many checks in a row only look again before anything is done: a power dip often fixes
/// itself, and restarting the audio session drops the Bluetooth stream.
pub const GRACE: u32 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// Something is wrong but may fix itself: look again shortly, touch nothing.
    Recheck,
    /// Switch the dongle off and on again (what a replug does).
    ResetDevice,
    /// Restart the audio session so it picks the dongle up again.
    RestartSession,
    /// Nothing worked; ask for a replug.
    GiveUp,
}

/// Decides what to do about a USB audio dongle that worked and then stopped, which is what a
/// power change on the battery board does. Each problem gets a few tries, then one request for a
/// replug, then silence until the dongle works again. A dongle that was never seen working, or
/// that is simply unplugged, is left alone.
#[derive(Default)]
pub struct AudioGuard {
    seen: bool,
    attempts: u32,
    streak: u32,
}

impl AudioGuard {
    pub fn observe(&mut self, health: Health) -> Option<Step> {
        if health.card && health.sink {
            self.seen = true;
            self.attempts = 0;
            self.streak = 0;
            return None;
        }
        if !self.seen {
            return None;
        }
        // A card without an output: restart the audio session twice, then reset the dongle
        // twice. A dongle with no card at all: reset it four times.
        let (limit, restarts) = if health.card {
            (4, 2)
        } else if health.device {
            (4, 0)
        } else {
            self.streak = 0;
            return None;
        };
        self.streak += 1;
        if self.streak <= GRACE {
            return Some(Step::Recheck);
        }
        self.attempts += 1;
        match self.attempts {
            n if n <= restarts => Some(Step::RestartSession),
            n if n <= limit => Some(Step::ResetDevice),
            n if n == limit + 1 => Some(Step::GiveUp),
            _ => None,
        }
    }

    /// The dongle refused to be reset: stop retrying until it works again (a replug).
    pub fn stuck(&mut self) {
        self.attempts = u32::MAX / 2;
    }
}

fn card_usbids() -> Vec<String> {
    let Ok(entries) = std::fs::read_dir("/proc/asound") else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|e| std::fs::read_to_string(e.path().join("usbid")).ok())
        .map(|t| t.trim().to_owned())
        .filter(|t| !t.is_empty())
        .collect()
}

/// The sysfs folder of a USB device with this `vendor:product` id.
fn usb_device_dir(usbid: &str) -> Option<std::path::PathBuf> {
    let (vendor, product) = usbid.split_once(':')?;
    std::fs::read_dir("/sys/bus/usb/devices")
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|p| {
            let read = |name: &str| std::fs::read_to_string(p.join(name)).unwrap_or_default();
            read("idVendor").trim() == vendor && read("idProduct").trim() == product
        })
}

fn sink_present() -> bool {
    Command::new("pactl")
        .args(["list", "short", "sinks"])
        .output()
        .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains("alsa_output.usb"))
}

/// Looks at the dongle: remembers its id while it works, and reports its health.
pub fn probe(remembered: &mut Option<String>) -> Health {
    let cards = card_usbids();
    let sink = sink_present();
    if remembered.is_none() {
        *remembered = cards.first().cloned();
    }
    let known = remembered.as_deref();
    Health {
        card: known.is_some_and(|id| cards.iter().any(|c| c == id)),
        device: known.is_some_and(|id| usb_device_dir(id).is_some()),
        sink,
    }
}

/// Restarting the audio session also drops the Bluetooth audio link, so once it is up again the
/// link to any connected phone or Mac is reopened from this side.
fn restart_session_and_reconnect() {
    restart_session();
    std::thread::sleep(Duration::from_secs(4));
    crate::bluetooth::restore_audio();
}

fn restart_session() {
    let _ = Command::new("systemctl")
        .args(["--user", "restart", "wireplumber"])
        .status();
}

/// Toggles the USB device off and on, like unplugging and replugging it. Needs passwordless sudo.
fn reset_device(usbid: &str) -> bool {
    let Some(dir) = usb_device_dir(usbid) else {
        return false;
    };
    let write = |value: &str| {
        let Ok(mut child) = Command::new("sudo")
            .arg("-n")
            .arg("tee")
            .arg(dir.join("authorized"))
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        else {
            return false;
        };
        if let Some(mut input) = child.stdin.take() {
            let _ = input.write_all(value.as_bytes());
        }
        child.wait().is_ok_and(|s| s.success())
    };
    let off = write("0");
    std::thread::sleep(std::time::Duration::from_secs(1));
    let on = write("1");
    std::thread::sleep(std::time::Duration::from_secs(3));
    off && on
}

/// A line from `udevadm monitor` saying a USB device or sound card appeared, went away or was
/// bound to / released from its driver.
pub fn is_hardware_event(line: &str) -> bool {
    let words: Vec<&str> = line.split_whitespace().collect();
    words
        .last()
        .is_some_and(|w| matches!(*w, "(usb)" | "(sound)"))
        && words
            .iter()
            .skip(1)
            .take(2)
            .any(|w| matches!(*w, "add" | "remove" | "bind" | "unbind"))
}

/// A line from `pactl subscribe` saying an audio output or card appeared or went away. Changes
/// (volume, state) and streams starting or stopping are left out: they happen all the time.
pub fn is_sink_event(line: &str) -> bool {
    let Some(rest) = line.strip_prefix("Event '") else {
        return false;
    };
    let Some((action, rest)) = rest.split_once('\'') else {
        return false;
    };
    matches!(action, "new" | "remove")
        && (rest.starts_with(" on sink #") || rest.starts_with(" on card #"))
}

/// Waits for an event, then for things to settle (a power change makes a burst of them), and
/// reports that one check is due. False when the watcher is gone.
pub fn settle_once(events: &mpsc::Receiver<()>, delay: Duration) -> bool {
    if events.recv().is_err() {
        return false;
    }
    std::thread::sleep(delay);
    while events.try_recv().is_ok() {}
    true
}

type Slot = Arc<Mutex<Option<Child>>>;
/// A helper program to follow, its arguments, and which of its lines matter.
type Source = (&'static str, &'static [&'static str], fn(&str) -> bool);

fn follow(
    program: &'static str,
    args: &'static [&'static str],
    keep: fn(&str) -> bool,
    events: mpsc::Sender<()>,
    slot: Slot,
    stop: Arc<AtomicBool>,
) {
    while !stop.load(Ordering::Relaxed) {
        let spawned = Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn();
        let Ok(mut child) = spawned else {
            std::thread::sleep(Duration::from_secs(30));
            continue;
        };
        let output = child.stdout.take();
        if let Ok(mut held) = slot.lock() {
            *held = Some(child);
        }
        if let Some(output) = output {
            for line in BufReader::new(output).lines().map_while(|l| l.ok()) {
                if keep(&line) && events.send(()).is_err() {
                    return;
                }
            }
        }
        if let Ok(mut held) = slot.lock() {
            if let Some(mut child) = held.take() {
                let _ = child.wait();
            }
        }
        std::thread::sleep(Duration::from_secs(5));
    }
}

/// Follows the kernel's hardware announcements and the audio server's output changes, and sends
/// a unit value for each one that matters. Stops (and ends its helper programs) when dropped.
pub struct EventWatch {
    slots: Vec<Slot>,
    stop: Arc<AtomicBool>,
}

impl EventWatch {
    pub fn start(events: mpsc::Sender<()>) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let sources: [Source; 2] = [
            (
                "udevadm",
                &[
                    "monitor",
                    "--udev",
                    "--subsystem-match=usb",
                    "--subsystem-match=sound",
                ],
                is_hardware_event,
            ),
            ("pactl", &["subscribe"], is_sink_event),
        ];
        let mut slots = Vec::new();
        for (program, args, keep) in sources {
            let slot: Slot = Arc::new(Mutex::new(None));
            slots.push(slot.clone());
            let (events, stop) = (events.clone(), stop.clone());
            std::thread::spawn(move || follow(program, args, keep, events, slot, stop));
        }
        Self { slots, stop }
    }
}

impl Drop for EventWatch {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        for slot in &self.slots {
            if let Ok(mut held) = slot.lock() {
                if let Some(child) = held.as_mut() {
                    let _ = child.kill();
                }
            }
        }
    }
}

/// One check: does what the guard decides and returns a note for the status line, if any, and
/// whether to look again soon (it acted, so see whether that worked).
pub fn guard_tick(
    guard: &mut AudioGuard,
    remembered: &mut Option<String>,
) -> Option<(Option<String>, bool)> {
    let health = probe(remembered);
    let note = |text: &str| Some(text.to_owned());
    match guard.observe(health)? {
        Step::Recheck => Some((None, true)),
        Step::ResetDevice => {
            let reset = remembered.as_deref().is_some_and(reset_device);
            if !reset {
                guard.stuck();
                return Some((
                    note("Audio dongle is stuck. Unplug it and plug it back in."),
                    false,
                ));
            }
            restart_session_and_reconnect();
            Some((note("Audio dongle dropped out; restarting it..."), true))
        }
        Step::RestartSession => {
            restart_session_and_reconnect();
            Some((
                note("Audio output dropped; restarting the audio session..."),
                true,
            ))
        }
        Step::GiveUp => Some((
            note("Audio dongle is not responding. Unplug it and plug it back in."),
            false,
        )),
    }
}
