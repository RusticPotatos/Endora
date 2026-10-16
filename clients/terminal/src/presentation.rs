use crate::protocol::clean;
use serde_json::Value;

fn label(value: &Value) -> String {
    clean(value.as_str().unwrap_or(""))
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(180)
        .collect()
}

/// Disclose observed action status without dumping tool inputs or results.
pub fn actions(value: &Value) -> String {
    let Some(items) = value["actions_taken"]
        .as_array()
        .filter(|items| !items.is_empty())
    else {
        return String::new();
    };
    let mut text = "\nActions:\n".to_owned();
    for item in items.iter().take(5) {
        let skill = label(&item["skill"]);
        let status = if item["confirmed"].as_bool() == Some(true) {
            "confirmed"
        } else {
            "unconfirmed"
        };
        text.push_str(&format!("  {skill}: {status}\n"));
    }
    if items.len() > 5 {
        text.push_str(&format!("  + {} more actions\n", items.len() - 5));
    }
    text
}

pub fn activity(value: &Value) -> String {
    let Some(items) = value.as_array() else {
        return "Activity unavailable".into();
    };
    if items.is_empty() {
        return "No recent activity".into();
    }
    items
        .iter()
        .take(20)
        .map(|item| format!("{}: {}\n", label(&item["kind"]), label(&item["summary"])))
        .collect()
}

/// Every service on the home screen, in display order (empty groups contribute nothing).
pub fn dash_services(value: &Value) -> Vec<&Value> {
    value["groups"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|g| g["services"].as_array().into_iter().flatten())
        .collect()
}

/// One key per service so it can be opened with a single press. Letters the console already
/// uses on this page (k lock, p pocket, u unlock, v reconcile) are skipped.
pub const DASH_KEYS: &str = "abcdefghijlmnoqrstwxyz0123456789";

/// The service a key opens, as an index into [`dash_services`].
pub fn dash_key_index(key: char) -> Option<usize> {
    DASH_KEYS
        .chars()
        .position(|k| k == key.to_ascii_lowercase())
}

/// Columns that fit in `width` for a group whose widest label is `widest` characters.
fn columns(widest: usize, width: usize) -> usize {
    (width / (widest + 5 + 2)).max(1)
}

/// The numbers shown after a service's name, e.g. `2 pending, 17 approved`.
fn numbers(service: &Value) -> String {
    let detail = label(&service["detail"]).replace(" | ", ", ");
    if detail.is_empty() {
        detail
    } else {
        format!("  {detail}")
    }
}

/// The widest "name  numbers" among the services of one group.
fn widest(services: &[Value]) -> usize {
    services
        .iter()
        .map(|s| (label(&s["name"]) + &numbers(s)).chars().count())
        .max()
        .unwrap_or(0)
}

/// The home screen's visual rows, left to right within each group, as flat service indices.
/// A row never spans two groups.
pub fn dash_rows(value: &Value, width: usize) -> Vec<Vec<usize>> {
    let mut rows = Vec::new();
    let mut index = 0;
    for group in value["groups"].as_array().into_iter().flatten() {
        let services = group["services"].as_array().map_or(&[][..], Vec::as_slice);
        if services.is_empty() {
            continue;
        }
        let cols = columns(widest(services), width);
        for chunk in (index..index + services.len())
            .collect::<Vec<_>>()
            .chunks(cols)
        {
            rows.push(chunk.to_vec());
        }
        index += services.len();
    }
    rows
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Move {
    Up,
    Down,
    Left,
    Right,
}

/// Moves the home-screen cursor. Left/Right step through the services in order; Up/Down keep
/// the column (clamped) in the neighbouring row, which may belong to the next group.
pub fn dash_move(rows: &[Vec<usize>], at: usize, direction: Move) -> usize {
    let last = rows.iter().flatten().max().copied().unwrap_or(0);
    match direction {
        Move::Left => at.saturating_sub(1),
        Move::Right => (at + 1).min(last),
        Move::Up | Move::Down => {
            let Some((row, col)) = rows
                .iter()
                .enumerate()
                .find_map(|(r, cells)| cells.iter().position(|&i| i == at).map(|c| (r, c)))
            else {
                return at;
            };
            let target = if direction == Move::Up {
                row.checked_sub(1)
            } else {
                Some(row + 1).filter(|r| *r < rows.len())
            };
            target.map_or(at, |r| rows[r][col.min(rows[r].len() - 1)])
        }
    }
}

/// The Homepage-style home screen: a status line, firing alerts, then each group as a grid
/// (`key mark name`; `+` running, `!` unhealthy, `-` stopped, `?` unknown) with a `>` cursor
/// on the selected service. Returns the text and the line the cursor is on, so the caller can
/// keep it in view.
/// A line for the passcode box when the device is not online, so it is clear how to fix that
/// before unlocking.
pub fn lock_hint(net: &str) -> Option<&'static str> {
    match net {
        "No Wi-Fi" => Some("No network | Esc opens Wi-Fi settings"),
        "Portal login needed" => Some("Wi-Fi login needed | Esc, then L"),
        "Joining" => Some("Joining Wi-Fi..."),
        _ => None,
    }
}

/// The top bar's right-hand side, like a phone: connection icons, clock, then battery.
pub fn status_right(icons: &str, time: &str, battery: &str) -> String {
    if battery.is_empty() {
        format!("{icons}  {time}")
    } else {
        format!("{icons}  {time}  {battery}")
    }
}

pub fn home(value: &Value, time: &str, selected: usize, width: usize) -> (String, usize) {
    let Some(groups) = value["groups"].as_array() else {
        return ("No dashboard data yet".into(), 0);
    };
    let total = dash_services(value).len();
    if total == 0 {
        return ("No dashboard data yet".into(), 0);
    }
    let selected = selected.min(total - 1);
    let mut head: Vec<String> = if time.is_empty() {
        Vec::new()
    } else {
        vec![time.to_owned()]
    };
    head.extend(value["stats"].as_array().into_iter().flatten().map(label));
    let mut text = head.join(" | ");
    text.push('\n');
    let (mut index, mut cursor_line) = (0, 0);
    for group in groups {
        let services = group["services"].as_array().map_or(&[][..], Vec::as_slice);
        if services.is_empty() {
            continue;
        }
        text.push_str(&format!("{}\n", label(&group["name"])));
        let name_width = widest(services);
        let cols = columns(name_width, width);
        for chunk in services.chunks(cols) {
            let mut row = String::new();
            for service in chunk {
                let mark = match service["state"].as_str() {
                    Some("running") => '+',
                    Some("unhealthy") => '!',
                    Some("stopped") => '-',
                    _ => '?',
                };
                if index == selected {
                    cursor_line = text.lines().count();
                }
                let cell = format!(
                    "{}{} {mark} {}",
                    if index == selected { '>' } else { ' ' },
                    DASH_KEYS.chars().nth(index).unwrap_or('.'),
                    label(&service["name"]) + &numbers(service)
                );
                row.push_str(&format!("{cell:<w$}  ", w = name_width + 5));
                index += 1;
            }
            text.push_str(row.trim_end());
            text.push('\n');
        }
    }
    (text, cursor_line)
}

/// The passcode as drawn in the lock box: a star per character, or (when asked) the text itself,
/// so a slip between upper and lower case can be seen. Long text keeps its last characters.
pub fn passcode_line(passcode: &str, reveal: bool) -> String {
    let count = passcode.chars().count();
    if !reveal {
        return "*".repeat(count.min(24));
    }
    if count <= 28 {
        return passcode.to_owned();
    }
    let tail: String = passcode.chars().skip(count - 26).collect();
    format!("..{tail}")
}

/// The line under the passcode: a caps-lock warning takes priority over the network note.
pub fn lock_notice(caps_lock: bool, network_note: Option<&str>) -> String {
    if caps_lock {
        "CAPS LOCK ON - Alt+C clears it".to_owned()
    } else {
        network_note.unwrap_or("").to_owned()
    }
}

/// A fixed-size box in the middle of the screen, as an OS lock screen would be.
pub fn lock_box(area: ratatui::layout::Rect) -> ratatui::layout::Rect {
    let width = area.width.min(36);
    let height = area.height.min(8);
    ratatui::layout::Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    )
}

/// The Music page's browse menu: each choice's key (the same key works without the menu) and name.
pub const MUSIC_MENU: [(char, &str); 10] = [
    ('/', "Search"),
    ('d', "Discover"),
    ('s', "Smart shuffle (Liked songs)"),
    ('o', "Play on another device"),
    ('l', "Playlists"),
    ('y', "Liked songs"),
    ('a', "Saved albums"),
    ('t', "Top artists"),
    ('k', "Top tracks"),
    ('r', "Recently played"),
];

/// The Music page: what is playing, the current list with a `>` cursor, and the keys.
pub fn music_view(
    now: Option<&crate::spotify::NowPlaying>,
    title: &str,
    items: &[crate::spotify::Item],
    selected: usize,
    signed_in: bool,
    note: &str,
    radio: bool,
) -> String {
    if !signed_in {
        return "Spotify is not set up yet.\nPress Y to sign in (you scan two QR codes),\nor run: endora-terminal music setup\n".into();
    }
    let mut text = String::from("Now playing\n");
    match now {
        Some(n) => {
            text.push_str(&format!("  {} - {}\n", n.title, n.artist));
            text.push_str(&format!(
                "  {} {} / {}  vol {}%  [{}]\n",
                if n.playing { "Playing" } else { "Paused" },
                crate::spotify::time_text(n.progress_ms),
                crate::spotify::time_text(n.duration_ms),
                n.volume.map_or_else(|| "?".to_owned(), |v| v.to_string()),
                n.device
            ));
        }
        None => text.push_str("  Nothing is playing\n"),
    }
    if !note.is_empty() {
        text.push_str(&format!("\n{note}\n"));
    }
    if items.is_empty() {
        // No list loaded: the page is a menu, one choice per line, like the home screen.
        text.push_str("\nPlayback\n");
        for (keys, what) in [
            ("Space", "Play or pause"),
            ("N", "Next song"),
            ("B", "Previous song"),
            ("+ -", "Volume"),
            ("H", "Like this song (save to Liked songs)"),
            ("X", "Dislike: skip, and less of this artist"),
        ] {
            text.push_str(&format!("  {keys:<6} {what}\n"));
        }
        text.push_str("\nBrowse\n");
        for (i, (key, label)) in MUSIC_MENU.iter().enumerate() {
            text.push_str(&format!(
                "{} {}  {label}\n",
                if i == selected { '>' } else { ' ' },
                key.to_ascii_uppercase()
            ));
        }
        text.push_str("\nUp/Down + Enter, or a letter\n");
    } else {
        text.push_str(&format!("\n{title}\n"));
        if radio {
            text.push_str("  H  Like (save to Liked songs)\n");
            text.push_str("  X  Skip, and less of this artist\n");
            text.push_str("  It keeps going: more songs are added as it plays.\n\n");
        }
        for (i, item) in items.iter().enumerate() {
            text.push_str(&format!(
                " {} {}\n",
                if i == selected { '>' } else { ' ' },
                label(&serde_json::Value::String(item.label.clone()))
            ));
        }
        text.push_str("\nEnter plays (an artist opens their albums)\nEsc back to the Music menu\n");
    }
    text
}

/// The Bluetooth page: state, what to press, the devices, and a live pairing countdown.
pub fn bluetooth_view(
    powered: bool,
    connected: &[crate::bluetooth::Device],
    paired: &[crate::bluetooth::Device],
    found: &[crate::bluetooth::Device],
    window_left: Option<u64>,
    selected: usize,
    note: &str,
) -> String {
    let mut text = if powered {
        let names: Vec<&str> = connected.iter().map(|d| d.name.as_str()).collect();
        if names.is_empty() {
            "Bluetooth is on\n".to_owned()
        } else {
            format!("Bluetooth is on | connected: {}\n", names.join(", "))
        }
    } else {
        "Bluetooth is off\n".to_owned()
    };
    if let Some(left) = window_left {
        text.push_str(&format!(
            "\nDISCOVERABLE as \"{}\": pair from your phone or Mac now ({left} s left)\nEsc cancels\n",
            crate::bluetooth::ALIAS
        ));
    }
    if !note.is_empty() {
        text.push_str(&format!("\n{note}\n"));
    }
    text.push_str("\nP pair a new device (2 minutes; keeps the ones already paired)\nF clear all pairings (asks first) | R reset Bluetooth | T turn on/off\nS find a speaker to play audio on | A sound test (left, right, both)\n");
    let list: &[crate::bluetooth::Device] = if found.is_empty() { paired } else { found };
    if !list.is_empty() {
        text.push_str(if found.is_empty() {
            "\nPaired devices (Enter connects, X forgets)\n"
        } else {
            "\nFound nearby (Enter pairs)\n"
        });
        for (i, d) in list.iter().enumerate() {
            let tag = if connected.iter().any(|c| c.mac.eq_ignore_ascii_case(&d.mac)) {
                "  (connected)"
            } else {
                ""
            };
            text.push_str(&format!(
                " {} {}{tag}\n",
                if i == selected { '>' } else { ' ' },
                label(&serde_json::Value::String(d.name.clone()))
            ));
        }
    }
    text
}

/// Applies one chat stream event to the chat text, returning a new status line if it changes
/// the status. Read as the protocol's event (ADR 0099): the node sends `step` while it works
/// and `error` when the exchange fails, which this client once ignored while waiting for a
/// `progress` event the node never sends.
pub fn on_stream(
    event: &endora_protocol::StreamEvent,
    chat: &mut String,
    stream_seen: &mut bool,
) -> Option<String> {
    use endora_protocol::StreamEvent;
    match event {
        StreamEvent::Token { text } => {
            chat.push_str(&crate::protocol::clean(text));
            *stream_seen = true;
            None
        }
        StreamEvent::Done { reply, actions } => {
            if !*stream_seen {
                chat.push_str(&crate::protocol::clean(&reply.text));
            }
            chat.push_str(&self::actions(actions));
            None
        }
        StreamEvent::Step { label, .. } => Some(if label.is_empty() {
            "Endora is working".to_owned()
        } else {
            format!("Endora is working: {}", crate::protocol::clean(label))
        }),
        StreamEvent::Error { message } => Some(format!(
            "Endora could not answer: {}",
            crate::protocol::clean(message)
        )),
        StreamEvent::Unknown => None,
    }
}

/// One line of a field, cleaned of terminal controls and bounded.
pub fn line(v: &Value) -> String {
    crate::protocol::clean(v.as_str().unwrap_or(""))
        .chars()
        .take(240)
        .collect()
}
/// A page's items.
pub fn items(v: &Value) -> &[Value] {
    v["items"].as_array().map(Vec::as_slice).unwrap_or(&[])
}
/// A page as text: its summary, then one row per item, the selected one marked.
pub fn display(v: &Value, selected: usize) -> String {
    let mut text = crate::protocol::clean(v["summary"].as_str().unwrap_or(""))
        .chars()
        .take(32 * 1024)
        .collect::<String>();
    text.push('\n');
    for (index, item) in items(v).iter().enumerate() {
        text.push_str(&format!(
            "{} {}{}  {}\n",
            if index == selected { ">" } else { " " },
            if item["favorite"] == true { "* " } else { "" },
            line(&item["label"]),
            line(&item["detail"])
        ));
    }
    if let Some(services) = v["services"].as_array() {
        for service in services {
            text.push_str(&format!(
                "{}: {}{}\n",
                line(&service["name"]),
                line(&service["state"]),
                service["total"]
                    .as_u64()
                    .map_or(String::new(), |n| format!(" ({n})"))
            ));
        }
    }
    text
}
