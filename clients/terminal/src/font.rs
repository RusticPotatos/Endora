//! Text size on the Linux console, which is set by the console font (the screen is a fixed
//! 720x720 grid of pixels, so a taller font means fewer, larger characters). The size is kept
//! between runs. Every level is a font file shipped with Raspberry Pi OS: the boot font is built
//! into the kernel and cannot be loaded back from a file, so Normal is an ordinary 8x16 font.
use crate::Result;
use std::process::{Command, Stdio};

pub const MIN: i8 = -1;
pub const MAX: i8 = 3;

pub fn step(level: i8, delta: i8) -> i8 {
    level.saturating_add(delta).clamp(MIN, MAX)
}

/// The console font for a level.
pub fn font_for(level: i8) -> &'static str {
    match level {
        i8::MIN..=-1 => "Lat15-Terminus12x6",
        0 => "Uni2-VGA16",
        1 => "Lat15-TerminusBold20x10",
        2 => "Lat15-TerminusBold24x12",
        _ => "Lat15-TerminusBold28x14",
    }
}

/// The font for showing QR codes: one of the few installed fonts with the half-block characters
/// QR codes are drawn with, and small enough (90 x 51 characters) for a code to fit.
pub const QR_FONT: &str = "Uni2-VGA14";

pub fn label(level: i8) -> &'static str {
    match level {
        i8::MIN..=-1 => "Small",
        0 => "Normal",
        1 => "Large",
        2 => "Larger",
        _ => "Largest",
    }
}

/// A saved level, tolerating anything: unreadable text means the default size.
pub fn parse_level(text: &str) -> i8 {
    text.trim()
        .parse::<i64>()
        .map_or(0, |n| n.clamp(i64::from(MIN), i64::from(MAX)) as i8)
}

pub fn load() -> i8 {
    crate::storage::Paths::state()
        .ok()
        .and_then(|p| p.read("text-size").ok())
        .map_or(0, |b| parse_level(&String::from_utf8_lossy(&b)))
}

pub fn save(level: i8) {
    if let Ok(paths) = crate::storage::Paths::state() {
        let _ = paths.write("text-size", level.to_string().as_bytes());
    }
}

/// Switches the console to this level's font. It must run on the console itself (the program's
/// own terminal); the screen then reports its new size, which the interface redraws for.
pub fn apply(level: i8) -> Result<()> {
    apply_font(font_for(level))
}

/// Switches the console to a named font (see [`apply`]).
pub fn apply_font(name: &str) -> Result<()> {
    let ok = Command::new("setfont")
        .arg(name)
        .stdin(Stdio::inherit())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    if ok {
        Ok(())
    } else {
        Err("Cannot change the text size on this screen".into())
    }
}
