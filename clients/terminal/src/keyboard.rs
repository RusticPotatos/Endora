//! Keyboard state the console cannot see in key events. Caps lock produces no character, so its
//! state is read from the keyboard's light, which the kernel exposes as a file.
use std::path::Path;

/// True when any keyboard caps-lock light under `leds` is on.
pub fn caps_lock_in(leds: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(leds) else {
        return false;
    };
    entries.flatten().any(|entry| {
        entry.file_name().to_string_lossy().ends_with("::capslock")
            && std::fs::read_to_string(entry.path().join("brightness"))
                .ok()
                .and_then(|text| text.trim().parse::<u32>().ok())
                .is_some_and(|value| value > 0)
    })
}

/// The program and arguments that turn caps lock off on the console.
pub fn caps_off_command() -> (&'static str, Vec<&'static str>) {
    ("setleds", vec!["-F", "-caps"])
}

/// Turns caps lock off. It has to run on the console itself (the program's own terminal).
pub fn clear_caps_lock() -> bool {
    let (program, args) = caps_off_command();
    std::process::Command::new(program)
        .args(args)
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

pub fn caps_lock_on() -> bool {
    caps_lock_in(Path::new("/sys/class/leds"))
}

/// Volume down (-1) or up (+1) for a raw key record from the keyboard's input device: the key
/// left of the trackpad (it sends Caps Lock) lowers the volume and the BlackBerry key (it sends
/// Super) raises it. A held key repeats; a release does nothing.
pub fn volume_key_step(record: &[u8]) -> Option<i32> {
    if record.len() != 24 {
        return None;
    }
    let kind = u16::from_ne_bytes(record[16..18].try_into().expect("checked length"));
    let code = u16::from_ne_bytes(record[18..20].try_into().expect("checked length"));
    let value = i32::from_ne_bytes(record[20..24].try_into().expect("checked length"));
    if kind != 1 || !matches!(value, 1 | 2) {
        return None;
    }
    match code {
        58 => Some(-1),
        125 => Some(1),
        _ => None,
    }
}

/// A console keymap that makes those two keys do nothing on the console itself, so they no longer
/// toggle caps lock; the app reads them directly for volume.
pub fn volume_keymap() -> String {
    let mut map = String::new();
    for code in [58, 125] {
        for modifiers in [
            "plain",
            "shift",
            "altgr",
            "shift altgr",
            "control",
            "alt",
            "shift control",
            "control alt",
            "shift alt",
        ] {
            map.push_str(&format!("{modifiers} keycode {code} = VoidSymbol\n"));
        }
    }
    map
}

/// The command that loads a keymap from standard input. The kernel only lets a privileged process
/// change modifier keys (Super is one), so it runs through passwordless `sudo`.
pub fn keymap_command() -> (&'static str, Vec<&'static str>) {
    ("sudo", vec!["-n", "loadkeys", "-"])
}

/// Applies the keymap. It has to run on the console itself: `loadkeys` finds the console among
/// its own streams, so stderr is left attached to the terminal (it only prints on an error).
pub fn apply_volume_keymap() -> bool {
    use std::io::Write;
    let (program, args) = keymap_command();
    let Ok(mut child) = std::process::Command::new(program)
        .args(args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::inherit())
        .spawn()
    else {
        return false;
    };
    if let Some(mut input) = child.stdin.take() {
        let _ = input.write_all(volume_keymap().as_bytes());
    }
    child.wait().is_ok_and(|s| s.success())
}

/// The command that changes the volume of the current output by 5% per step (at most four steps
/// at once), never above 100%.
pub fn volume_command(steps: i32) -> (&'static str, Vec<&'static str>) {
    let change = match steps.clamp(-4, 4) {
        1 => "5%+",
        2 => "10%+",
        3 => "15%+",
        4 => "20%+",
        -1 => "5%-",
        -2 => "10%-",
        -3 => "15%-",
        _ => "20%-",
    };
    (
        "wpctl",
        vec!["set-volume", "-l", "1.0", "@DEFAULT_AUDIO_SINK@", change],
    )
}

/// The current output volume as a percentage, from `wpctl get-volume` output ("Volume: 0.45").
pub fn parse_volume(text: &str) -> Option<u32> {
    let value: f32 = text.split_whitespace().nth(1)?.parse().ok()?;
    Some((value * 100.0).round().clamp(0.0, 150.0) as u32)
}
