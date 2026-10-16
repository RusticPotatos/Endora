use std::{process::Command, time::Duration};
#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Travel,
    Desk,
}
#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    #[default]
    Awake,
    Dim,
    Pocket,
}
#[derive(Default)]
pub struct Power {
    pub mode: Mode,
    pub pocket_seconds: u64,
    current: Stage,
}
impl Power {
    pub fn stage(&mut self, idle: Duration, busy: bool) -> Stage {
        let (dim, pocket) = if self.mode == Mode::Travel {
            (
                60,
                if self.pocket_seconds == 0 {
                    180
                } else {
                    self.pocket_seconds.clamp(60, 600)
                },
            )
        } else {
            (600, 1800)
        };
        self.current = if busy {
            Stage::Awake
        } else if idle.as_secs() >= pocket {
            Stage::Pocket
        } else if idle.as_secs() >= dim {
            Stage::Dim
        } else {
            Stage::Awake
        };
        self.current
    }
    pub fn pocket(&mut self) {
        self.current = Stage::Pocket;
    }
    pub fn wake(&mut self) -> bool {
        let consume = self.current == Stage::Pocket;
        self.current = Stage::Awake;
        consume
    }
}
/// The command that restarts the whole device cleanly. Needs passwordless sudo.
pub fn reboot_command() -> (&'static str, Vec<&'static str>) {
    ("sudo", vec!["-n", "systemctl", "reboot"])
}

/// Restarts the device.
pub fn reboot() -> bool {
    let (program, args) = reboot_command();
    Command::new(program).args(args).spawn().is_ok()
}

pub fn backlight(stage: Stage) -> bool {
    if !std::path::Path::new("/usr/local/libexec/endora-backlight").is_file() {
        return false;
    }
    Command::new("sudo")
        .args([
            "-n",
            "/usr/local/libexec/endora-backlight",
            match stage {
                Stage::Awake => "on",
                Stage::Dim => "dim",
                Stage::Pocket => "off",
            },
        ])
        .status()
        .is_ok_and(|s| s.success())
}

/// Private timing-only history; never changes policy without user acceptance.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Usage {
    pub samples: Vec<u32>,
    pub pocket_seconds: u64,
}
impl Default for Usage {
    fn default() -> Self {
        Self {
            samples: Vec::new(),
            pocket_seconds: 180,
        }
    }
}
impl Usage {
    pub fn load() -> Self {
        let mut value = crate::storage::Paths::state()
            .ok()
            .and_then(|p| p.read("idle-timing").ok())
            .and_then(|b| serde_json::from_slice::<Self>(&b).ok())
            .unwrap_or_default();
        value.samples.truncate(32);
        value.pocket_seconds = value.pocket_seconds.clamp(60, 600);
        value
    }
    pub fn save(&self) -> crate::Result<()> {
        crate::storage::Paths::state()?.write(
            "idle-timing",
            &serde_json::to_vec(self).map_err(|_| "Cannot encode idle timing")?,
        )
    }
    pub fn record(&mut self, duration: Duration) {
        self.samples.push(duration.as_secs().min(86400) as u32);
        if self.samples.len() > 32 {
            self.samples.remove(0);
        }
    }
    pub fn suggestion(&self) -> Option<u64> {
        if self.samples.len() < 8 {
            return None;
        }
        let quick = self.samples.iter().filter(|s| **s < 30).count();
        let long = self.samples.iter().filter(|s| **s > 600).count();
        let suggested = if quick * 2 >= self.samples.len() {
            300
        } else if long * 2 >= self.samples.len() {
            120
        } else {
            return None;
        };
        (suggested != self.pocket_seconds).then_some(suggested)
    }
    pub fn apply(&mut self) {
        if let Some(value) = self.suggestion() {
            self.pocket_seconds = value;
        }
    }
}
