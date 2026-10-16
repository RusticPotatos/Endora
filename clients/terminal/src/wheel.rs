/// Linux consoles use the input device instead of xterm mouse controls.
pub fn terminal_mouse_supported(term: &str, console: bool) -> bool {
    !console && !matches!(term, "linux" | "dumb")
}

pub fn is_console() -> bool {
    #[cfg(target_os = "linux")]
    {
        std::fs::read_link("/proc/self/fd/0")
            .ok()
            .and_then(|p| p.to_str().map(str::to_owned))
            .is_some_and(|p| {
                p.strip_prefix("/dev/tty")
                    .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
            })
    }
    #[cfg(not(target_os = "linux"))]
    false
}

/// Decode only ordinary wheel events from the ARM64 Linux input ABI.
pub fn wheel_delta(record: &[u8]) -> i32 {
    if record.len() != 24
        || record[16..18] != 2u16.to_ne_bytes()
        || record[18..20] != 8u16.to_ne_bytes()
    {
        return 0;
    }
    i32::from_ne_bytes(record[20..24].try_into().expect("checked record length"))
}

/// Motion and a pressed pointer button count as activity, even below a scroll step.
pub fn activity_event(record: &[u8]) -> bool {
    if record.len() != 24 {
        return false;
    }
    let kind = u16::from_ne_bytes(record[16..18].try_into().expect("checked length"));
    let code = u16::from_ne_bytes(record[18..20].try_into().expect("checked length"));
    let value = i32::from_ne_bytes(record[20..24].try_into().expect("checked length"));
    (kind == 2 && matches!(code, 0 | 1 | 8) && value != 0)
        || (kind == 1 && code == 272 && value == 1)
}

/// A press of the pointer's main button (the Q20 trackpad click), not its release.
pub fn click_event(record: &[u8]) -> bool {
    if record.len() != 24 {
        return false;
    }
    let kind = u16::from_ne_bytes(record[16..18].try_into().expect("checked length"));
    let code = u16::from_ne_bytes(record[18..20].try_into().expect("checked length"));
    let value = i32::from_ne_bytes(record[20..24].try_into().expect("checked length"));
    kind == 1 && code == 272 && value == 1
}

/// Pressed (true) or released (false) for the pointer's right button, which is what the Q20's
/// hang-up key sends. Auto-repeat and every other event give nothing.
pub fn right_button_event(record: &[u8]) -> Option<bool> {
    if record.len() != 24 {
        return None;
    }
    let kind = u16::from_ne_bytes(record[16..18].try_into().expect("checked length"));
    let code = u16::from_ne_bytes(record[18..20].try_into().expect("checked length"));
    let value = i32::from_ne_bytes(record[20..24].try_into().expect("checked length"));
    match (kind, code, value) {
        (1, 273, 1) => Some(true),
        (1, 273, 0) => Some(false),
        _ => None,
    }
}

/// How long the hang-up key is held to restart the device.
pub const RESTART_HOLD: std::time::Duration = std::time::Duration::from_secs(3);

/// Times how long a button has been held down.
#[derive(Default)]
pub struct HoldTimer {
    since: Option<std::time::Instant>,
}

impl HoldTimer {
    pub fn set(&mut self, down: bool, now: std::time::Instant) {
        if !down {
            self.since = None;
        } else if self.since.is_none() {
            self.since = Some(now);
        }
    }

    pub fn held(&self, now: std::time::Instant) -> Option<std::time::Duration> {
        self.since.map(|since| now.saturating_duration_since(since))
    }
}

pub fn scroll_by(position: u16, delta: i32, bottom: u16) -> u16 {
    (i64::from(position) - i64::from(delta) * 3).clamp(0, i64::from(bottom)) as u16
}

/// Q20's optical control reports pointer motion rather than wheel rotation.
/// Accumulate small movements so diagonal jitter does not jump the history.
#[derive(Default)]
pub struct Q20Scroll {
    vertical: i64,
    horizontal: i64,
}

impl Q20Scroll {
    /// Sideways pointer motion as left (negative) / right (positive) steps. A step needs
    /// more travel than a vertical one, so the small sideways drift of a vertical swipe
    /// does not move the home-screen cursor.
    pub fn horizontal(&mut self, record: &[u8]) -> i32 {
        if record.len() != 24
            || record[16..18] != 2u16.to_ne_bytes()
            || record[18..20] != 0u16.to_ne_bytes()
        {
            return 0;
        }
        let motion = i32::from_ne_bytes(record[20..24].try_into().expect("checked record length"));
        self.horizontal += i64::from(motion);
        let steps = self.horizontal / 16;
        self.horizontal %= 16;
        steps as i32
    }

    pub fn event(&mut self, record: &[u8]) -> i32 {
        if record.len() != 24 || record[16..18] != 2u16.to_ne_bytes() {
            return 0;
        }
        if record[18..20] == 8u16.to_ne_bytes() {
            self.vertical = 0;
            return wheel_delta(record);
        }
        if record[18..20] != 1u16.to_ne_bytes() {
            return 0;
        }
        let motion = i32::from_ne_bytes(record[20..24].try_into().expect("checked record length"));
        self.vertical -= i64::from(motion);
        let steps = self.vertical / 12;
        self.vertical %= 12;
        steps as i32
    }
}

/// The Q20 keyboard's own input device, opened to read the volume keys.
#[cfg(all(target_os = "linux", target_pointer_width = "64"))]
fn open_keys() -> Option<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::read_dir("/dev/input/by-id")
        .ok()?
        .flatten()
        .find(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            name.contains("HACKBerryPiQ20") && name.ends_with("event-kbd")
        })
        .and_then(|e| {
            std::fs::OpenOptions::new()
                .read(true)
                .custom_flags(rustix::fs::OFlags::NONBLOCK.bits() as i32)
                .open(e.path())
                .ok()
        })
}

#[derive(Default)]
pub struct ConsoleWheel {
    volume: i32,
    activity: bool,
    clicks: u32,
    hold: HoldTimer,
    sideways: i32,
    #[cfg(all(target_os = "linux", target_pointer_width = "64"))]
    file: Option<std::fs::File>,
    #[cfg(all(target_os = "linux", target_pointer_width = "64"))]
    keys: Option<std::fs::File>,
    #[cfg(all(target_os = "linux", target_pointer_width = "64"))]
    control: Q20Scroll,
    #[cfg(all(target_os = "linux", target_pointer_width = "64"))]
    retry: Option<std::time::Instant>,
}

impl ConsoleWheel {
    pub fn close(&mut self) {
        self.hold = HoldTimer::default();
        #[cfg(all(target_os = "linux", target_pointer_width = "64"))]
        {
            self.file = None;
            self.keys = None;
            self.control = Q20Scroll::default();
        }
    }

    pub fn open() -> Self {
        #[cfg(all(target_os = "linux", target_pointer_width = "64"))]
        {
            use std::os::unix::fs::OpenOptionsExt;
            if is_console() {
                if let Ok(entries) = std::fs::read_dir("/dev/input/by-id") {
                    for entry in entries.flatten() {
                        let name = entry.file_name().to_string_lossy().into_owned();
                        if name.contains("HACKBerryPiQ20") && name.ends_with("event-mouse") {
                            let file = std::fs::OpenOptions::new()
                                .read(true)
                                .custom_flags(rustix::fs::OFlags::NONBLOCK.bits() as i32)
                                .open(entry.path())
                                .ok();
                            return Self {
                                keys: open_keys(),
                                volume: 0,
                                file,
                                control: Q20Scroll::default(),
                                activity: false,
                                clicks: 0,
                                hold: HoldTimer::default(),
                                sideways: 0,
                                retry: None,
                            };
                        }
                    }
                }
            }
        }
        Self::default()
    }

    pub fn poll(&mut self) -> i32 {
        #[cfg(all(target_os = "linux", target_pointer_width = "64"))]
        {
            use std::io::Read;
            if self.file.is_none()
                && self
                    .retry
                    .is_none_or(|when| std::time::Instant::now() >= when)
            {
                let activity = self.activity;
                *self = Self::open();
                self.activity |= activity;
                self.retry = Some(std::time::Instant::now() + std::time::Duration::from_secs(5));
            }
            if let Some(keys) = &mut self.keys {
                let mut record = [0; 24];
                for _ in 0..64 {
                    match keys.read(&mut record) {
                        Ok(24) => {
                            if let Some(step) = crate::keyboard::volume_key_step(&record) {
                                self.volume = self.volume.saturating_add(step);
                            }
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                        _ => {
                            self.keys = None;
                            break;
                        }
                    }
                }
            }
            if let Some(file) = &mut self.file {
                let mut delta = 0i32;
                let mut record = [0; 24];
                for _ in 0..64 {
                    match file.read(&mut record) {
                        Ok(24) => {
                            self.activity |= activity_event(&record);
                            if click_event(&record) {
                                self.clicks = self.clicks.saturating_add(1);
                            }
                            if let Some(down) = right_button_event(&record) {
                                self.hold.set(down, std::time::Instant::now());
                            }
                            delta = delta.saturating_add(self.control.event(&record));
                            self.sideways = self
                                .sideways
                                .saturating_add(self.control.horizontal(&record));
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                        _ => {
                            self.file = None;
                            break;
                        }
                    }
                }
                return delta;
            }
        }
        0
    }

    /// Left/right steps accumulated since the last call.
    pub fn take_sideways(&mut self) -> i32 {
        std::mem::take(&mut self.sideways)
    }

    /// Volume steps from the two keys left of the trackpad since the last call.
    pub fn take_volume(&mut self) -> i32 {
        std::mem::take(&mut self.volume)
    }

    /// How long the hang-up key has been held down, if it is.
    pub fn right_held(&self) -> Option<std::time::Duration> {
        self.hold.held(std::time::Instant::now())
    }

    /// Whether the trackpad was clicked since the last call.
    pub fn take_click(&mut self) -> bool {
        std::mem::take(&mut self.clicks) > 0
    }

    pub fn take_activity(&mut self) -> bool {
        std::mem::take(&mut self.activity)
    }
}
