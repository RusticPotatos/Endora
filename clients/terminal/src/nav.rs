//! Navigation: the menu and the Settings page, laid out like a phone's Settings. Every menu and
//! section letter is the first letter of what it opens, Esc always goes back one level, and the
//! arrows plus Enter work everywhere a letter does.
use crossterm::event::KeyCode;

/// Where a menu item goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// A page by its index (0 Home, 2 Controls, 3 TV & Movies, 4 NAS, 5 Jobs, 6 Settings,
    /// 7 Music, 8 Bluetooth). Slot 1 was the chat, retired with it (ADR 0136).
    Page(usize),
    /// Settings, at its list or straight inside one section.
    Settings(Option<Section>),
    Lock,
    Pocket,
}

pub struct MenuItem {
    pub key: char,
    pub label: &'static str,
    pub target: Target,
}

/// The menu: the pages first, then shortcuts.
pub const MENU: &[MenuItem] = &[
    MenuItem {
        key: 'h',
        label: "Home",
        target: Target::Page(0),
    },
    MenuItem {
        key: 'c',
        label: "Controls",
        target: Target::Page(2),
    },
    MenuItem {
        key: 't',
        label: "TV & Movies",
        target: Target::Page(3),
    },
    MenuItem {
        key: 'm',
        label: "Music",
        target: Target::Page(7),
    },
    MenuItem {
        key: 'n',
        label: "NAS",
        target: Target::Page(4),
    },
    MenuItem {
        key: 'j',
        label: "Jobs",
        target: Target::Page(5),
    },
    MenuItem {
        key: 's',
        label: "Settings",
        target: Target::Settings(None),
    },
    MenuItem {
        key: 'b',
        label: "Bluetooth",
        target: Target::Page(8),
    },
    MenuItem {
        key: 'w',
        label: "Wi-Fi",
        target: Target::Settings(Some(Section::WiFi)),
    },
    MenuItem {
        key: 'l',
        label: "Lock",
        target: Target::Lock,
    },
    MenuItem {
        key: 'p',
        label: "Pocket (screen off)",
        target: Target::Pocket,
    },
];

/// How many of the menu items are pages; the rest are shortcuts, shown after a gap.
const MENU_PAGES: usize = 8;

/// The menu item for a letter, in either case.
pub fn menu_key(c: char) -> Option<usize> {
    let c = c.to_ascii_lowercase();
    MENU.iter().position(|m| m.key == c)
}

pub fn menu_view(selected: usize) -> String {
    let mut text = String::from("Menu\n\n");
    for (i, item) in MENU.iter().enumerate() {
        if i == MENU_PAGES {
            text.push('\n');
        }
        let cursor = if i == selected { '>' } else { ' ' };
        text.push_str(&format!(
            "{cursor} {}  {}\n",
            item.key.to_ascii_uppercase(),
            item.label
        ));
    }
    text.push_str("\nUp/Down + Enter, or a letter | Esc closes\n");
    text
}

/// A Settings section.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    WiFi,
    Bluetooth,
    Sound,
    Display,
    Network,
    Accounts,
    Tools,
    Info,
    Power,
}

pub struct SectionItem {
    pub key: char,
    pub label: &'static str,
    pub section: Section,
}

const SECTIONS: &[SectionItem] = &[
    SectionItem {
        key: 'w',
        label: "Wi-Fi",
        section: Section::WiFi,
    },
    SectionItem {
        key: 'b',
        label: "Bluetooth",
        section: Section::Bluetooth,
    },
    SectionItem {
        key: 's',
        label: "Sound",
        section: Section::Sound,
    },
    SectionItem {
        key: 'd',
        label: "Display",
        section: Section::Display,
    },
    SectionItem {
        key: 'n',
        label: "Network",
        section: Section::Network,
    },
    SectionItem {
        key: 'a',
        label: "Accounts",
        section: Section::Accounts,
    },
    SectionItem {
        key: 't',
        label: "Tools",
        section: Section::Tools,
    },
    SectionItem {
        key: 'i',
        label: "Info",
        section: Section::Info,
    },
    SectionItem {
        key: 'p',
        label: "Power",
        section: Section::Power,
    },
];

pub fn sections() -> &'static [SectionItem] {
    SECTIONS
}

/// What a key in Settings does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Move the cursor to this row.
    Select(usize),
    /// Go back one level.
    Back,
    /// Show a section's rows.
    Open(Section),
    /// Go to a page (Bluetooth has its own).
    Page(usize),
    /// One of the device actions the console already knows, by its original Connect-page key.
    Legacy(char),
    /// Make the text larger (1) or smaller (-1).
    TextSize(i8),
    /// Restart the device (asks first).
    Restart,
    /// Play the left, right, both sound test.
    SoundTest,
    /// Change what shows after unlocking (Home, the menu, or the last page used).
    StartScreen,
    /// Connect (true) or disconnect (false) the VPN by hand.
    Vpn(bool),
    /// Check whether traffic goes through the VPN.
    VpnTest,
    /// Read the network details again.
    NetRefresh,
}

pub struct Row {
    pub key: char,
    pub label: &'static str,
    pub action: Action,
}

const fn row(key: char, label: &'static str, action: Action) -> Row {
    Row { key, label, action }
}

const WIFI: &[Row] = &[
    row('c', "Choose a network", Action::Legacy('w')),
    row('l', "Log in to a Wi-Fi page (text)", Action::Legacy('o')),
    row(
        'g',
        "Log in to a Wi-Fi page (graphical)",
        Action::Legacy('p'),
    ),
    row('a', "Advanced (nmtui)", Action::Legacy('n')),
];
const SOUND: &[Row] = &[row('t', "Test left and right", Action::SoundTest)];
const DISPLAY: &[Row] = &[
    row('+', "Larger text", Action::TextSize(1)),
    row('-', "Smaller text", Action::TextSize(-1)),
    row(
        'o',
        "Screen-off timing: Travel or Desk",
        Action::Legacy('z'),
    ),
    row(
        'a',
        "Apply the suggested screen-off time",
        Action::Legacy('a'),
    ),
    row('r', "Reset screen-off history", Action::Legacy('i')),
    row(
        's',
        "Start screen: Home, Menu or Last used",
        Action::StartScreen,
    ),
];
const NETWORK: &[Row] = &[
    row('c', "Connect VPN", Action::Vpn(true)),
    row('d', "Disconnect VPN", Action::Vpn(false)),
    row(
        't',
        "Test VPN (is traffic going through it?)",
        Action::VpnTest,
    ),
    row('r', "Refresh these details", Action::NetRefresh),
    row('h', "Switch home / away profile", Action::Legacy('t')),
];
const ACCOUNTS: &[Row] = &[
    row('e', "Endora sign-in", Action::Legacy('s')),
    row('p', "Passcode and encryption", Action::Legacy('e')),
    row('r', "Remote profile", Action::Legacy('f')),
    row('s', "Spotify sign-in", Action::Legacy('y')),
    row('b', "Browser shortcuts", Action::Legacy('v')),
];
const TOOLS: &[Row] = &[
    row('s', "Shell", Action::Legacy('l')),
    row('x', "SSH", Action::Legacy('x')),
    row('w', "Web browser (text)", Action::Legacy('r')),
    row('b', "Bookmarks", Action::Legacy('b')),
    row('d', "Desktop (Alt+Back returns)", Action::Legacy('h')),
];
const INFO: &[Row] = &[row('d', "Run diagnostics", Action::Legacy('d'))];
const POWER: &[Row] = &[
    row('r', "Restart", Action::Restart),
    row('s', "Shut down", Action::Legacy('g')),
    row('q', "Exit to the recovery shell", Action::Legacy('q')),
];

/// What shows after unlocking.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartScreen {
    Home,
    Menu,
    /// The page you were on last.
    Last,
}

impl StartScreen {
    /// A saved choice; anything unreadable means Home.
    pub fn parse(text: &str) -> Self {
        match text.trim() {
            "menu" => Self::Menu,
            "last" => Self::Last,
            _ => Self::Home,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Home => "home",
            Self::Menu => "menu",
            Self::Last => "last",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Home => "Home",
            Self::Menu => "Menu",
            Self::Last => "Last used",
        }
    }

    /// The next choice, round and round.
    pub fn next(self) -> Self {
        match self {
            Self::Home => Self::Menu,
            Self::Menu => Self::Last,
            Self::Last => Self::Home,
        }
    }
}

/// The page to show after unlocking, and whether the menu is open over it.
pub fn start_view(choice: StartScreen, last_page: Option<usize>) -> (usize, bool) {
    match choice {
        StartScreen::Home => (0, false),
        StartScreen::Menu => (0, true),
        StartScreen::Last => (
            last_page.filter(|p| *p < 9 && *p != CHAT).unwrap_or(0),
            false,
        ),
    }
}

/// The page that was the chat, retired with it (ADR 0136): never offered, never reached.
const CHAT: usize = 1;

/// The page Tab moves to from `page`: the next one, around the nine, skipping the chat's.
pub fn next_page(page: usize) -> usize {
    let next = (page + 1) % 9;
    if next == CHAT {
        return next + 1;
    }
    next
}

/// The rows inside a section. Bluetooth has none: it opens its own page.
pub fn rows(section: Section) -> &'static [Row] {
    match section {
        Section::WiFi => WIFI,
        Section::Bluetooth => &[],
        Section::Sound => SOUND,
        Section::Display => DISPLAY,
        Section::Network => NETWORK,
        Section::Accounts => ACCOUNTS,
        Section::Tools => TOOLS,
        Section::Info => INFO,
        Section::Power => POWER,
    }
}

fn top_action(item: &SectionItem) -> Action {
    match item.section {
        Section::Bluetooth => Action::Page(8),
        section => Action::Open(section),
    }
}

/// What a key does in Settings, at the list (`section` is `None`) or inside a section.
pub fn settings_key(section: Option<Section>, selected: usize, code: KeyCode) -> Option<Action> {
    let entries: Vec<(char, Action)> = match section {
        None => SECTIONS.iter().map(|s| (s.key, top_action(s))).collect(),
        Some(section) => rows(section).iter().map(|r| (r.key, r.action)).collect(),
    };
    let last = entries.len().saturating_sub(1);
    match code {
        KeyCode::Up => Some(Action::Select(selected.saturating_sub(1))),
        KeyCode::Down => Some(Action::Select((selected + 1).min(last))),
        KeyCode::Enter => entries.get(selected).map(|e| e.1),
        KeyCode::Esc => Some(Action::Back),
        KeyCode::Char(c) => {
            let c = match c.to_ascii_lowercase() {
                '=' => '+',
                c => c,
            };
            entries.iter().find(|e| e.0 == c).map(|e| e.1)
        }
        _ => None,
    }
}

/// What the Settings screens show next to their rows.
pub struct SettingsStatus {
    pub wifi: String,
    pub bluetooth: String,
    pub text_size: String,
    pub screen: String,
    pub profile: String,
    pub vpn: String,
    pub version: String,
    /// What shows after unlocking.
    pub start: String,
    /// Network details, one per line (Settings > Network).
    pub network: Vec<String>,
}

fn short(text: &str) -> String {
    let text: String = text.chars().filter(|c| !c.is_control()).collect();
    if text.chars().count() > 26 {
        format!("{}...", text.chars().take(23).collect::<String>())
    } else {
        text
    }
}

fn summary(section: Section, s: &SettingsStatus) -> String {
    match section {
        Section::WiFi => short(&s.wifi),
        Section::Bluetooth => short(&s.bluetooth),
        Section::Display => short(&format!("Text {}, {}", s.text_size, s.screen)),
        Section::Network => short(&format!("{}, VPN {}", s.profile, s.vpn)),
        Section::Info => short(&s.version),
        _ => String::new(),
    }
}

fn section_note(section: Section, s: &SettingsStatus) -> String {
    match section {
        Section::WiFi => format!("Now: {}", short(&s.wifi)),
        Section::Bluetooth => "Pair and connect devices on the Bluetooth page.".into(),
        Section::Sound => {
            "Volume: the key left of the trackpad is down,\nthe BlackBerry key is up.".into()
        }
        Section::Display => format!(
            "Text {} | Screen-off {} | Start {}",
            s.text_size, s.screen, s.start
        ),
        Section::Network => format!("Profile {} | VPN {}", s.profile, short(&s.vpn)),
        Section::Accounts => "Sign-ins and the device passcode.".into(),
        Section::Tools => "Each opens full screen; leave it to return here.".into(),
        Section::Info => format!("Endora terminal {}", short(&s.version)),
        Section::Power => "Holding the hang-up key 3 s also restarts.".into(),
    }
}

/// The Settings list, or one section's rows, with a `>` cursor on the selected row.
pub fn settings_view(section: Option<Section>, selected: usize, status: &SettingsStatus) -> String {
    let mut text = String::new();
    match section {
        None => {
            text.push_str("Settings\n\n");
            for (i, item) in SECTIONS.iter().enumerate() {
                let cursor = if i == selected { '>' } else { ' ' };
                text.push_str(&format!(
                    "{cursor} {}  {:<12}{}\n",
                    item.key.to_ascii_uppercase(),
                    item.label,
                    summary(item.section, status)
                ));
            }
            text.push_str("\nUp/Down + Enter, or a letter | Esc back\n");
        }
        Some(section) => {
            let label = SECTIONS
                .iter()
                .find(|s| s.section == section)
                .map_or("", |s| s.label);
            text.push_str(&format!("Settings > {label}\n"));
            text.push_str(&format!("{}\n", section_note(section, status)));
            if section == Section::Network {
                for line in &status.network {
                    text.push_str(&format!("  {line}\n"));
                }
            }
            text.push('\n');
            for (i, row) in rows(section).iter().enumerate() {
                let cursor = if i == selected { '>' } else { ' ' };
                text.push_str(&format!(
                    "{cursor} {}  {}\n",
                    row.key.to_ascii_uppercase(),
                    row.label
                ));
            }
            text.push_str("\nUp/Down + Enter, or a letter | Esc back\n");
        }
    }
    text
}

/// The keys a trackpad movement stands for, so every screen treats it exactly like the arrows:
/// lists move their cursor, text pages scroll. Up for positive steps, at most four per pass so a
/// fast flick does not jump to the end; a click is Enter, after the movement.
pub fn trackpad_keys(delta: i32, clicked: bool) -> Vec<KeyCode> {
    let key = if delta > 0 {
        KeyCode::Up
    } else {
        KeyCode::Down
    };
    let mut keys = vec![key; delta.unsigned_abs().min(4) as usize];
    if clicked {
        keys.push(KeyCode::Enter);
    }
    keys
}
