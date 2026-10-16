//! Power and activity history, so what drains the battery can be diagnosed later.
//!
//! One small sample a minute: load, memory, temperature, undervoltage, the screen stage and,
//! when the kernel exposes a battery, its level. Nothing here records conversation text,
//! network names or locations. The file is private (`0600`), bounded, and local to the device.
use crate::Result;
use serde::{Deserialize, Serialize};
use std::{
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Sample {
    pub t: u64,
    pub stage: String,
    pub mode: String,
    pub page: u8,
    pub load1: f32,
    pub mem: u8,
    pub temp: Option<f32>,
    pub undervolt: Option<bool>,
    pub bat: Option<u8>,
    pub charging: Option<bool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Battery {
    pub pct: u8,
    pub charging: bool,
}

pub fn parse_loadavg(text: &str) -> Option<f32> {
    text.split_whitespace().next()?.parse().ok()
}

/// Percent of memory in use, from `/proc/meminfo`.
pub fn parse_meminfo(text: &str) -> Option<u8> {
    let field = |name: &str| -> Option<f64> {
        text.lines()
            .find_map(|l| l.strip_prefix(name))?
            .split_whitespace()
            .next()?
            .parse()
            .ok()
    };
    let (total, available) = (field("MemTotal:")?, field("MemAvailable:")?);
    (total > 0.0).then(|| {
        (100.0 * (1.0 - available / total))
            .round()
            .clamp(0.0, 100.0) as u8
    })
}

/// The first power-supply device that is a battery with a sane capacity.
pub fn read_battery(dir: &Path) -> Option<Battery> {
    let mut entries: Vec<_> = std::fs::read_dir(dir).ok()?.flatten().collect();
    entries.sort_by_key(std::fs::DirEntry::file_name);
    entries.into_iter().find_map(|entry| {
        let read = |name: &str| std::fs::read_to_string(entry.path().join(name)).ok();
        if read("type")?.trim() != "Battery" {
            return None;
        }
        let pct: u8 = read("capacity")?
            .trim()
            .parse()
            .ok()
            .filter(|p| *p <= 100)?;
        let charging = read("status").is_some_and(|s| s.trim() == "Charging");
        Some(Battery { pct, charging })
    })
}

/// Minutes of battery left at the recent drain rate; `None` when charging, when there is too
/// little history to trust, or when the level has not dropped.
pub fn estimate_minutes_left(samples: &[Sample], now: u64) -> Option<u32> {
    let recent: Vec<&Sample> = samples
        .iter()
        .filter(|s| s.t + 3 * 3600 >= now && s.bat.is_some())
        .collect();
    // Only the stretch since the last charge counts.
    let start = recent
        .iter()
        .rposition(|s| s.charging == Some(true))
        .map_or(0, |i| i + 1);
    let run = &recent[start..];
    if recent.last()?.charging == Some(true) || run.len() < 3 {
        return None;
    }
    let (first, last) = (run.first()?, run.last()?);
    let span = last.t.checked_sub(first.t)? as f64;
    let dropped = f64::from(first.bat?) - f64::from(last.bat?);
    if span < 1800.0 || dropped <= 0.0 {
        return None;
    }
    let rate_per_hour = dropped / (span / 3600.0);
    let minutes = f64::from(last.bat?) / rate_per_hour * 60.0;
    Some(minutes.round().min(7.0 * 24.0 * 60.0) as u32)
}

pub fn battery_text(battery: Option<Battery>, minutes_left: Option<u32>) -> String {
    // This board's power bank reports nothing; then nothing is shown rather than "n/a".
    let Some(b) = battery else {
        return String::new();
    };
    if b.charging {
        return format!("Bat {}% charging", b.pct);
    }
    match minutes_left {
        Some(m) if m >= 60 => format!("Bat {}% ~{}h{}m", b.pct, m / 60, m % 60),
        Some(m) => format!("Bat {}% ~{m}m", b.pct),
        None => format!("Bat {}%", b.pct),
    }
}

/// An append-only JSON-lines file that is rotated in half when it grows past `max_bytes`.
pub struct History {
    path: PathBuf,
    max_bytes: u64,
}

impl History {
    pub fn new(path: PathBuf, max_bytes: u64) -> Self {
        Self { path, max_bytes }
    }

    pub fn standard() -> Result<Self> {
        let paths = crate::storage::Paths::state()?;
        Ok(Self::new(
            paths.root.join("power-history.jsonl"),
            4 * 1024 * 1024,
        ))
    }

    pub fn append(&self, sample: &Sample) -> Result<()> {
        use std::os::unix::fs::OpenOptionsExt;
        if std::fs::symlink_metadata(&self.path).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err("History path must not be a symlink".into());
        }
        if std::fs::metadata(&self.path).is_ok_and(|m| m.len() > self.max_bytes) {
            self.rotate()?;
        }
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open(&self.path)
            .map_err(|_| "Cannot open power history")?;
        let mut line = serde_json::to_string(sample).map_err(|_| "Cannot encode sample")?;
        line.push('\n');
        file.write_all(line.as_bytes())
            .map_err(|_| "Cannot write power history".into())
    }

    fn rotate(&self) -> Result<()> {
        use std::os::unix::fs::OpenOptionsExt;
        let text = std::fs::read_to_string(&self.path).map_err(|_| "Cannot read power history")?;
        let lines: Vec<&str> = text.lines().collect();
        let keep = lines[lines.len() / 2..].join("\n") + "\n";
        let temp = self.path.with_extension("tmp");
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .mode(0o600)
            .open(&temp)
            .map_err(|_| "Cannot rotate power history")?;
        file.write_all(keep.as_bytes())
            .map_err(|_| "Cannot rotate power history")?;
        std::fs::rename(&temp, &self.path).map_err(|_| "Cannot rotate power history".into())
    }

    pub fn load(&self) -> Vec<Sample> {
        std::fs::read_to_string(&self.path)
            .unwrap_or_default()
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect()
    }
}

/// A reading of this device right now. Parts the system does not expose are `None`.
pub fn take_sample(stage: &str, mode: &str, page: u8) -> Sample {
    let read = |p: &str| std::fs::read_to_string(p).ok();
    let undervolt = std::fs::read_dir("/sys/class/hwmon").ok().and_then(|d| {
        d.flatten().find_map(|e| {
            let name = std::fs::read_to_string(e.path().join("name")).ok()?;
            (name.trim() == "rpi_volt").then(|| {
                std::fs::read_to_string(e.path().join("in0_lcrit_alarm"))
                    .ok()
                    .map(|v| v.trim() == "1")
            })?
        })
    });
    let battery = read_battery(Path::new("/sys/class/power_supply"));
    Sample {
        t: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        stage: stage.into(),
        mode: mode.into(),
        page,
        load1: read("/proc/loadavg")
            .and_then(|t| parse_loadavg(&t))
            .unwrap_or(0.0),
        mem: read("/proc/meminfo")
            .and_then(|t| parse_meminfo(&t))
            .unwrap_or(0),
        temp: read("/sys/class/thermal/thermal_zone0/temp")
            .and_then(|t| t.trim().parse::<f32>().ok())
            .map(|m| m / 1000.0),
        undervolt,
        bat: battery.map(|b| b.pct),
        charging: battery.map(|b| b.charging),
    }
}

/// A plain-language summary of where time and charge went, with suggestions.
pub fn report(samples: &[Sample]) -> String {
    let Some(first) = samples.first() else {
        return "No history yet. The device records one sample a minute while the app runs.".into();
    };
    let last = samples.last().unwrap_or(first);
    let hours = last.t.saturating_sub(first.t) as f64 / 3600.0;
    let mut out = format!("History: {} samples over {hours:.1} h\n", samples.len());
    let mut stages: Vec<&str> = Vec::new();
    for s in samples {
        if !stages.contains(&s.stage.as_str()) {
            stages.push(&s.stage);
        }
    }
    let weight = |i: usize| -> f64 {
        samples
            .get(i + 1)
            .map_or(60, |n| n.t.saturating_sub(samples[i].t).min(300)) as f64
    };
    let total: f64 = (0..samples.len()).map(weight).sum::<f64>().max(1.0);
    let mut have_battery = false;
    let mut busy_when_dark = None;
    for stage in &stages {
        let idx: Vec<usize> = (0..samples.len())
            .filter(|&i| samples[i].stage == *stage)
            .collect();
        let secs: f64 = idx.iter().map(|&i| weight(i)).sum();
        let n = idx.len() as f32;
        let load = idx.iter().map(|&i| samples[i].load1).sum::<f32>() / n;
        let temps: Vec<f32> = idx.iter().filter_map(|&i| samples[i].temp).collect();
        let temp = (!temps.is_empty()).then(|| temps.iter().sum::<f32>() / temps.len() as f32);
        let (mut dropped, mut dt) = (0.0f64, 0.0f64);
        for &i in &idx {
            if let (Some(a), Some(b)) = (samples.get(i), samples.get(i + 1)) {
                if let (Some(x), Some(y)) = (a.bat, b.bat) {
                    if a.charging != Some(true) && b.charging != Some(true) && x >= y {
                        dropped += f64::from(x - y);
                        dt += weight(i);
                    }
                }
            }
        }
        out.push_str(&format!(
            "  {stage}: {:.0} min ({:.0}%), load {load:.2}",
            secs / 60.0,
            100.0 * secs / total
        ));
        if let Some(t) = temp {
            out.push_str(&format!(", {t:.0} C"));
        }
        if dt >= 600.0 {
            have_battery = true;
            out.push_str(&format!(", drain {:.1} %/h", dropped / (dt / 3600.0)));
        }
        out.push('\n');
        if *stage == "pocket" && load > 0.5 {
            busy_when_dark = Some(load);
        }
    }
    let under = samples.iter().filter(|s| s.undervolt == Some(true)).count();
    if under > 0 {
        out.push_str(&format!(
            "The supply reported undervoltage in {under} samples: it sagged. Charge sooner or check the cable.\n"
        ));
    }
    if let Some(load) = busy_when_dark {
        out.push_str(&format!(
            "The device was busy while the screen was off (load {load:.2}): look for background work.\n"
        ));
    }
    if !have_battery {
        out.push_str(
            "No battery sensor data: history records activity only. A battery gauge is needed for percent, time left and per-state drain.\n",
        );
    }
    out
}
