use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers,
        MouseEventKind,
    },
    execute,
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};
use endora_terminal::presentation::{display, items, line};
use endora_terminal::{
    Result, bluetooth,
    client::Client,
    composer, network,
    portable::{self, Settings},
    power::{self, Mode, Power, Stage},
    presentation::{Move, dash_key_index, dash_move, dash_rows, dash_services, home, status_right},
    protocol::clean,
    spotify, telemetry, vpn,
    wheel::ConsoleWheel,
};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Layout},
    widgets::{Block, Borders, Paragraph, Wrap},
};
use serde_json::{Value, json};
use std::{
    io,
    process::Command,
    time::{Duration, Instant},
};
use tokio::sync::mpsc;

const NAMES: [&str; 9] = [
    "Home",
    // 1 was the chat, retired with it (ADR 0136): never shown.
    "",
    "Controls",
    "TV & Movies",
    "NAS",
    "Jobs",
    "Settings",
    "Music",
    "Bluetooth",
];
struct Screen {
    mouse: bool,
}
impl Screen {
    fn enter() -> Result<Self> {
        terminal::enable_raw_mode().map_err(|_| "Cannot enter terminal mode")?;
        let mouse = endora_terminal::wheel::terminal_mouse_supported(
            &std::env::var("TERM").unwrap_or_default(),
            endora_terminal::wheel::is_console(),
        );
        if execute!(io::stdout(), EnterAlternateScreen)
            .and_then(|_| {
                if mouse {
                    execute!(io::stdout(), EnableMouseCapture)
                } else {
                    Ok(())
                }
            })
            .is_err()
        {
            let _ = terminal::disable_raw_mode();
            return Err("Cannot open screen".into());
        }
        Ok(Self { mouse })
    }
}
impl Drop for Screen {
    fn drop(&mut self) {
        let _ = terminal::disable_raw_mode();
        if self.mouse {
            let _ = execute!(io::stdout(), DisableMouseCapture);
        }
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
    }
}
/// Work done for the Music page, off the input loop.
enum Music {
    Now(Result<Option<spotify::NowPlaying>>),
    Items(String, Result<Vec<spotify::Item>>),
    Done(Result<String>),
    /// The Discover radio started with these songs.
    Radio(Result<Vec<spotify::Item>>),
    /// More songs were queued for the Discover radio.
    RadioMore(Result<Vec<spotify::Item>>),
    /// Smart shuffle started: its name, your songs it draws from, and what is playing.
    Smart(Result<(String, Vec<spotify::Item>, Vec<spotify::Item>)>),
}
enum MusicAction {
    Poll,
    Toggle(bool),
    Skip(bool),
    Volume(u8),
    Play(spotify::Item),
    Search(String),
    Playlists,
    Liked,
    Recent,
    Albums,
    TopArtists,
    TopTracks,
    /// Start the Discover radio, avoiding these artists and songs already heard.
    RadioStart {
        avoid: Vec<String>,
        played: Vec<String>,
    },
    /// Queue the next batch: new songs, mixed into your own when it is a smart shuffle.
    RadioMore {
        avoid: Vec<String>,
        played: Vec<String>,
        base: Option<Vec<spotify::Item>>,
    },
    /// Smart shuffle a playlist (or Liked songs when `None`), mixing in new songs.
    SmartStart {
        source: Option<spotify::Item>,
        name: String,
        avoid: Vec<String>,
        played: Vec<String>,
    },
    /// Save a song to Liked songs.
    Like(String),
    /// List the devices playback can move to.
    Devices,
    /// Move playback to this device (by id).
    Transfer(String),
    /// Play these songs in order.
    PlayTracks(Vec<String>),
    /// An artist's albums.
    Open(spotify::Item),
}
type Player = std::sync::Arc<tokio::sync::Mutex<Option<spotify::Spotify>>>;

/// `node`: the unlocked client, which Endora's Spotify sign-in is borrowed through.
async fn music_work(player: Player, node: Option<Client>, action: MusicAction) -> Music {
    let mut guard = player.lock().await;
    if guard.is_none() {
        match spotify::Spotify::load(node.as_ref()).await {
            Ok(session) => *guard = Some(session),
            Err(e) => {
                return match action {
                    MusicAction::Poll => Music::Now(Err(e)),
                    _ => Music::Done(Err(e)),
                };
            }
        }
    }
    let Some(sp) = guard.as_mut() else {
        return Music::Done(Err("Spotify is not set up".into()));
    };
    let done = |r: Result<()>, ok: &str| Music::Done(r.map(|()| ok.to_owned()));
    match action {
        MusicAction::Poll => Music::Now(sp.now().await),
        MusicAction::Toggle(playing) => done(
            if playing {
                sp.pause().await
            } else {
                sp.resume().await
            },
            if playing { "Paused" } else { "Playing" },
        ),
        MusicAction::Skip(next) => done(sp.skip(next).await, if next { "Next" } else { "Back" }),
        MusicAction::Volume(v) => done(sp.volume(v).await, "Volume set"),
        MusicAction::Play(item) => done(sp.play(&item).await, "Playing"),
        MusicAction::Search(q) => Music::Items(format!("Results for \"{q}\""), sp.search(&q).await),
        MusicAction::Playlists => Music::Items("Your playlists".into(), sp.playlists().await),
        MusicAction::Liked => Music::Items("Liked songs".into(), sp.liked().await),
        MusicAction::Recent => Music::Items("Recently played".into(), sp.recent().await),
        MusicAction::Albums => Music::Items("Your saved albums".into(), sp.saved_albums().await),
        MusicAction::TopArtists => Music::Items("Your top artists".into(), sp.top_artists().await),
        MusicAction::TopTracks => Music::Items("Your top tracks".into(), sp.top_tracks().await),
        MusicAction::RadioStart { avoid, played } => {
            Music::Radio(match sp.discover_tracks(&avoid, &played).await {
                Ok(tracks) => {
                    let uris: Vec<String> = tracks.iter().map(|t| t.uri.clone()).collect();
                    sp.play_tracks(&uris).await.map(|()| tracks)
                }
                Err(e) => Err(e),
            })
        }
        MusicAction::RadioMore {
            avoid,
            played,
            base,
        } => {
            let seed = radio_seed();
            Music::RadioMore(match sp.discover_tracks(&avoid, &played).await {
                Ok(fresh) => {
                    let batch = match base {
                        Some(base) => {
                            // Your songs not heard yet (all of them again once nearly used up).
                            let left: Vec<spotify::Item> = base
                                .iter()
                                .filter(|t| !played.contains(&t.uri))
                                .cloned()
                                .collect();
                            let pool = if left.len() < 10 { base } else { left };
                            let mut mix = spotify::smart_mix(&pool, &fresh, 3, seed);
                            mix.truncate(20);
                            mix
                        }
                        None => fresh,
                    };
                    let uris: Vec<String> = batch.iter().map(|t| t.uri.clone()).collect();
                    match sp.queue_all(&uris).await {
                        Ok(n) if n > 0 => Ok(batch),
                        Ok(_) => Err("Could not add more songs to the queue".into()),
                        Err(e) => Err(e),
                    }
                }
                Err(e) => Err(e),
            })
        }
        MusicAction::SmartStart {
            source,
            name,
            avoid,
            played,
        } => {
            let base = match &source {
                Some(playlist) => sp.playlist_tracks(playlist).await,
                None => sp.liked_sample().await,
            };
            Music::Smart(match base {
                Ok(base) => {
                    let fresh = sp
                        .discover_tracks(&avoid, &played)
                        .await
                        .unwrap_or_default();
                    let mix = spotify::smart_mix(&base, &fresh, 3, radio_seed());
                    if mix.is_empty() {
                        Err("No songs to shuffle there".into())
                    } else {
                        let uris: Vec<String> = mix.iter().map(|t| t.uri.clone()).collect();
                        sp.play_tracks(&uris).await.map(|()| (name, base, mix))
                    }
                }
                Err(e) => Err(e),
            })
        }
        MusicAction::Like(uri) => done(sp.like(&uri).await, "Saved to Liked songs"),
        MusicAction::Devices => Music::Items(
            "Play on (Enter moves the music there)".into(),
            sp.devices().await.map(|d| spotify::device_items(&d)),
        ),
        MusicAction::Transfer(id) => Music::Done(
            sp.transfer(&id)
                .await
                .map(|name| format!("Playing on {name}")),
        ),
        MusicAction::PlayTracks(uris) => done(sp.play_tracks(&uris).await, "Playing"),
        MusicAction::Open(artist) => Music::Items(
            format!("Albums by {}", artist.label.trim_start_matches("Artist: ")),
            sp.artist_albums(&artist).await,
        ),
    }
}
fn radio_seed() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(1, |d| d.as_nanos() as u64)
}

/// The Music page lists that keep going by themselves (the Discover radio and smart shuffle).
fn is_radio_title(title: &str) -> bool {
    title.starts_with("Discover radio") || title.starts_with("Smart shuffle")
}

/// Work and results for the Bluetooth page, off the input loop.
enum Bt {
    State {
        powered: bool,
        paired: Vec<bluetooth::Device>,
        connected: Vec<bluetooth::Device>,
    },
    Window(Option<u64>),
    Note(String),
    Found(Vec<bluetooth::Device>),
}
type Flag = std::sync::Arc<std::sync::atomic::AtomicBool>;

fn bt_snapshot() -> Bt {
    Bt::State {
        powered: bluetooth::powered(),
        paired: bluetooth::paired(),
        connected: bluetooth::connected(),
    }
}

/// Opens a pairing window without touching existing pairings (many devices can be paired). A
/// device that pairs is trusted and connected from this side too, which is what makes it show up
/// as a speaker on a phone or Mac. `cancel` closes the window early.
fn bt_pair_window(tx: mpsc::Sender<Update>, cancel: Flag, busy: Flag) {
    use std::sync::atomic::Ordering::Relaxed;
    busy.store(true, Relaxed);
    let send = |e: Bt| {
        let _ = tx.blocking_send(Update::Bt(e));
    };
    send(Bt::Note("Opening the pairing window...".into()));
    let known = bluetooth::paired();
    let before = known.len();
    let mut settle = bluetooth::Settle::default();
    let mut told = false;
    let result = bluetooth::pairing_window(
        120,
        |message| send(Bt::Note(message.to_owned())),
        |left| {
            send(Bt::Window(Some(left)));
            if !told {
                told = true;
                let names: Vec<String> = bluetooth::adapters().into_iter().map(|a| a.1).collect();
                if names.len() > 1 {
                    send(Bt::Note(format!(
                        "{} adapters are open: pick one of \"{}\" on your device.",
                        names.len(),
                        names.join("\", \"")
                    )));
                }
            }
            let (trust, done) = settle.tick(bluetooth::paired().len(), before);
            if trust {
                bluetooth::trust_paired();
                for d in bluetooth::new_devices(&known, &bluetooth::paired()) {
                    let note = if bluetooth::connect(&d).is_ok() {
                        format!(
                            "Paired and connected: {}. Pick \"{}\" in its sound settings.",
                            d.name,
                            bluetooth::ALIAS
                        )
                    } else {
                        format!(
                            "Paired: {}. If it is not listed as a speaker, reconnect it from its Bluetooth settings.",
                            d.name
                        )
                    };
                    send(Bt::Note(note));
                }
            }
            done || cancel.load(Relaxed)
        },
    );
    send(Bt::Window(None));
    if let Err(e) = result {
        send(Bt::Note(e));
    }
    send(bt_snapshot());
    busy.store(false, Relaxed);
}

fn bt_task(
    tx: mpsc::Sender<Update>,
    busy: Flag,
    work: impl FnOnce() -> Option<String> + Send + 'static,
) {
    use std::sync::atomic::Ordering::Relaxed;
    busy.store(true, Relaxed);
    if let Some(note) = work() {
        let _ = tx.blocking_send(Update::Bt(Bt::Note(note)));
    }
    let _ = tx.blocking_send(Update::Bt(bt_snapshot()));
    busy.store(false, Relaxed);
}

enum Update {
    /// A line for the status bar from a background check.
    Notice(String),
    /// The Wi-Fi device's details (Settings > Network).
    NetDetails(endora_terminal::network::NetDetails),
    /// Something changed in the audio hardware or server: check the dongle.
    AudioCheck,
    Bt(Bt),
    Music(Music),
    Read(u64, usize, Result<Value>),
    Action(u64, Result<Value>),
    Network(&'static str),
    /// Tunnel state, whether the node answers, and the gateway fingerprint.
    Link(vpn::Vpn, bool, Option<String>, bool),
}
/// Local wall-clock time, refreshed at most every 20 seconds.
fn local_time() -> String {
    static CACHE: std::sync::Mutex<Option<(Instant, String)>> = std::sync::Mutex::new(None);
    let Ok(mut cache) = CACHE.lock() else {
        return "--:--".into();
    };
    if let Some((at, text)) = cache
        .as_ref()
        .filter(|(at, _)| at.elapsed() < Duration::from_secs(20))
    {
        let _ = at;
        return text.clone();
    }
    let text = Command::new("date")
        .arg("+%-I:%M %p")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map_or("--:--".into(), |t| t.trim().to_owned());
    *cache = Some((Instant::now(), text.clone()));
    text
}
fn encoded(s: &str) -> String {
    s.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}
fn read(
    client: &Option<Client>,
    page: usize,
    query: &str,
    epoch: u64,
    catalog: bool,
    detail: Option<&str>,
    tx: &mpsc::Sender<Update>,
) -> Option<tokio::task::JoinHandle<()>> {
    let c = client.clone()?;
    let tx = tx.clone();
    let path = match page {
        0 => "/v1/terminal/dashboard".into(),
        2 => {
            if catalog {
                "/v1/terminal/home/catalog".into()
            } else {
                "/v1/terminal/home".into()
            }
        }
        3 => {
            if query.is_empty() {
                "/v1/terminal/media/requests".into()
            } else {
                format!("/v1/terminal/media?query={}", encoded(query))
            }
        }
        4 => "/v1/terminal/containers".into(),
        5 => "/v1/terminal/jobs".into(),
        _ => return None,
    };
    let path = detail.map_or(path, str::to_owned);
    Some(tokio::spawn(async move {
        let result = c.get(&path).await;
        let _ = tx.send(Update::Read(epoch, page, result)).await;
    }))
}
struct Confirm {
    body: Value,
    label: String,
}

pub async fn run() -> Result<()> {
    std::panic::set_hook(Box::new(|_| {
        let _ = terminal::disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        eprintln!("Endora stopped; use SSH recovery");
    }));
    let mut settings = Settings::load()?;
    let mut client = portable::locked(&settings).ok();
    let mut unlocked = false;
    // Columns the home screen last drew at, so Up/Down can move between rows.
    let mut dash_width = 76usize;
    let mut restarting = false;
    // Settings > Network: the Wi-Fi details, and a manual VPN choice that pauses auto switching.
    let mut net_details = endora_terminal::network::NetDetails::default();
    let mut vpn_hold_until: Option<Instant> = None;
    // What shows after unlocking, and the page last used (kept between runs).
    let read_state = |name: &str| {
        endora_terminal::storage::Paths::state()
            .and_then(|p| p.read(name))
            .map(|b| String::from_utf8_lossy(&b).into_owned())
            .unwrap_or_default()
    };
    let mut start_screen = endora_terminal::nav::StartScreen::parse(&read_state("start-screen"));
    let last_page: Option<usize> = read_state("last-page").trim().parse().ok();
    let mut saved_page = last_page;
    // Trackpad movement and clicks, turned into arrow keys and Enter for the key handling below.
    let mut pending_keys: std::collections::VecDeque<KeyCode> = std::collections::VecDeque::new();
    // Navigation: the menu's cursor, where Settings is, and where Esc returns to.
    let mut menu_sel = 0usize;
    let mut menu_shown = false;
    let mut settings_section: Option<endora_terminal::nav::Section> = None;
    let mut settings_sel = 0usize;
    let mut back_page: Option<usize> = None;
    let mut flow_return: Option<usize> = None;
    let mut reveal_passcode = false;
    let mut caps_on = false;
    let mut caps_checked = Instant::now();
    // The centered passcode box; shown until the first unlock after start (or an explicit lock).
    let mut prompt: Option<zeroize::Zeroizing<String>> =
        Some(zeroize::Zeroizing::new(String::new()));
    let mut page = 0;
    let mut menu = false;
    let mut selected = [0usize; 9];
    let mut data: Vec<Value> = (0..9).map(|_| json!({})).collect();
    let mut cursor = 0;
    let mut search = String::new();
    // Media: a result has been picked with Up/Down since the last edit or search.
    let mut picked = false;
    let mut scroll = [0u16; 9];
    let mut bottom = 0;
    let mut busy = false;
    let mut reading = false;
    let mut confirm: Option<Confirm> = None;
    let mut home_catalog = false;
    let mut detail: Option<(usize, String)> = None;
    let mut status = "Locked | U unlock | Esc menu".to_owned();
    let mut net = "Checking Wi-Fi";
    let mut epoch = 0u64;
    let mut tasks = Vec::new();
    let (tx, mut rx) = mpsc::channel(64);
    let mut guard = Some(Screen::enter()?);
    // The two keys left of the trackpad become volume down and up (they stop being caps lock).
    let _ = endora_terminal::keyboard::apply_volume_keymap();
    let mut last_volume = Instant::now();
    let mut text_size = endora_terminal::font::load();
    if text_size != 0 {
        let _ = endora_terminal::font::apply(text_size);
    }
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))
        .map_err(|_| "Cannot initialize screen")?;
    terminal.clear().map_err(|_| "Cannot clear screen")?;
    let mut wheel = ConsoleWheel::open();
    let mut dirty = true;
    let mut last_input = Instant::now();
    let mut last_read = Instant::now();
    let mut next_read = Instant::now();
    let mut next_net = Instant::now();
    // Watches the USB audio dongle: a power change on the battery board resets it.
    let mut next_audio = Instant::now() + Duration::from_secs(10);
    let audio_state = std::sync::Arc::new(std::sync::Mutex::new((
        endora_terminal::audio::AudioGuard::default(),
        None::<String>,
    )));
    let audio_busy: Flag = Flag::default();
    // Hardware and audio-server announcements trigger a check a few seconds after they settle;
    // the timer is only a slow safety net.
    let (audio_events, audio_listener) = std::sync::mpsc::channel();
    let _audio_watch = endora_terminal::audio::EventWatch::start(audio_events);
    {
        let tx = tx.clone();
        std::thread::spawn(move || {
            while endora_terminal::audio::settle_once(&audio_listener, Duration::from_secs(3)) {
                if tx.blocking_send(Update::AudioCheck).is_err() {
                    break;
                }
            }
        });
    }
    let mut vpn_state = vpn::Vpn::default();
    let mut bt_policy = bluetooth::Policy::default();
    let mut bt_off_by_hand = false;
    let mut bt_keep_ticks = 0u32;
    let mut bt_connected = false;
    let player: Player = std::sync::Arc::new(tokio::sync::Mutex::new(None));
    let mut now_playing: Option<spotify::NowPlaying> = None;
    let mut music_items: Vec<spotify::Item> = Vec::new();
    let mut music_title = String::new();
    // The Discover radio, whether its next batch is being fetched, and when it started.
    let mut radio: Option<spotify::Radio> = None;
    let mut radio_fetching = false;
    let mut radio_started = Instant::now();
    // Your own songs a smart shuffle draws from (none for the Discover radio).
    let mut radio_base: Option<Vec<spotify::Item>> = None;
    // Artists disliked (X) this session: the radio and smart shuffle leave them out.
    let mut disliked: Vec<String> = Vec::new();
    // Space auto-repeats when held; play/pause toggles once per press.
    let mut last_toggle = Instant::now() - Duration::from_secs(5);
    // The list to come back to after opening an artist's albums.
    let mut music_prev: Option<(String, Vec<spotify::Item>)> = None;
    let mut music_note = String::new();
    let mut next_music = Instant::now();
    let mut bt_powered = true;
    let mut bt_paired: Vec<bluetooth::Device> = Vec::new();
    let mut bt_conn: Vec<bluetooth::Device> = Vec::new();
    let mut bt_found: Vec<bluetooth::Device> = Vec::new();
    let mut bt_window: Option<u64> = None;
    let mut bt_note = String::new();
    let mut next_bt = Instant::now();
    let bt_cancel: Flag = Flag::default();
    let bt_busy: Flag = Flag::default();
    let mut portal_offered = false;
    let mut home_id = vpn::load_home();
    let mut unreachable_ticks = 0u32;
    // Power history: one sample a minute, kept in memory for the last three hours so the
    // battery estimate does not need to reread the file.
    let power_log = telemetry::History::standard().ok();
    let now_secs = || {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    };
    let mut recent: Vec<telemetry::Sample> = power_log
        .as_ref()
        .map(|h| h.load())
        .unwrap_or_default()
        .into_iter()
        .filter(|s| s.t + 3 * 3600 >= now_secs())
        .collect();
    let mut battery: Option<telemetry::Battery> = None;
    let mut minutes_left: Option<u32> = None;
    let mut next_sample = Instant::now();
    let mut failures = 0u32;
    let mut power = Power::default();
    let mut usage = power::Usage::load();
    power.pocket_seconds = usage.pocket_seconds;
    power.mode = if settings.desk {
        Mode::Desk
    } else {
        Mode::Travel
    };
    let mut pocket_since: Option<Instant> = None;
    let mut stage = Stage::Awake;
    loop {
        tasks.retain(|t: &tokio::task::JoinHandle<()>| !t.is_finished());
        while let Ok(update) = rx.try_recv() {
            match update {
                Update::Bt(event) => match event {
                    Bt::State {
                        powered,
                        paired,
                        connected,
                    } => {
                        bt_powered = powered;
                        bt_paired = paired;
                        bt_conn = connected;
                        next_bt = Instant::now() + Duration::from_secs(3);
                    }
                    Bt::Window(w) => bt_window = w,
                    Bt::Note(n) => bt_note = n,
                    Bt::Found(f) => {
                        bt_found = f;
                        selected[8] = 0;
                    }
                },
                Update::Music(event) => {
                    next_music = Instant::now() + Duration::from_secs(5);
                    match event {
                        Music::Now(Ok(now)) => {
                            now_playing = now;
                            // Follow the Discover radio: top it up on its last song, and end it
                            // once something else plays (after a moment for playback to start).
                            if let Some(n) = now_playing.as_ref().filter(|n| !n.uri.is_empty()) {
                                let mut stop = false;
                                if let Some(r) = radio.as_mut() {
                                    if r.is_ours(&n.uri) {
                                        r.playing(&n.uri);
                                        if r.needs_more(&n.uri) && !radio_fetching {
                                            radio_fetching = true;
                                            let action = MusicAction::RadioMore {
                                                avoid: disliked.clone(),
                                                played: r.played().to_vec(),
                                                base: radio_base.clone(),
                                            };
                                            let (tx, player, node) =
                                                (tx.clone(), player.clone(), client.clone());
                                            tasks.push(tokio::spawn(async move {
                                                let _ = tx
                                                    .send(Update::Music(
                                                        music_work(player, node, action).await,
                                                    ))
                                                    .await;
                                            }));
                                        }
                                    } else if radio_started.elapsed() > Duration::from_secs(15) {
                                        stop = true;
                                    }
                                }
                                if stop {
                                    radio = None;
                                    radio_base = None;
                                    radio_fetching = false;
                                }
                            }
                            if music_note.starts_with("Spotify")
                                || music_note.starts_with("No active")
                                || music_note == spotify::SPEAKER_MISSING
                            {
                                music_note.clear();
                            }
                        }
                        Music::Now(Err(e)) => music_note = e,
                        Music::Items(title, Ok(list)) => {
                            music_note = if list.is_empty() {
                                "Nothing found".into()
                            } else {
                                String::new()
                            };
                            music_title = title;
                            music_items = list;
                            selected[7] = 0;
                        }
                        Music::Radio(Ok(tracks)) => {
                            radio = Some(spotify::Radio::new(
                                tracks.iter().map(|t| t.uri.clone()).collect(),
                            ));
                            radio_started = Instant::now();
                            radio_fetching = false;
                            radio_base = None;
                            music_title = "Discover radio".into();
                            music_items = tracks;
                            selected[7] = 0;
                            music_note = "Discover radio is playing".into();
                            next_music = Instant::now() + Duration::from_secs(2);
                        }
                        Music::RadioMore(Ok(tracks)) => {
                            radio_fetching = false;
                            if let Some(r) = radio.as_mut() {
                                r.extend(tracks.iter().map(|t| t.uri.clone()).collect());
                                if is_radio_title(&music_title) {
                                    music_items.extend(tracks);
                                }
                            }
                        }
                        Music::Smart(Ok((name, base, mix))) => {
                            radio = Some(spotify::Radio::new(
                                mix.iter().map(|t| t.uri.clone()).collect(),
                            ));
                            radio_base = Some(base);
                            radio_started = Instant::now();
                            radio_fetching = false;
                            music_title = format!("Smart shuffle: {name}");
                            music_items = mix;
                            selected[7] = 0;
                            music_note = "Smart shuffle is playing".into();
                            next_music = Instant::now() + Duration::from_secs(2);
                        }
                        Music::Radio(Err(e)) | Music::RadioMore(Err(e)) | Music::Smart(Err(e)) => {
                            radio_fetching = false;
                            music_note = e;
                        }
                        Music::Items(_, Err(e)) | Music::Done(Err(e)) => music_note = e,
                        Music::Done(Ok(msg)) => {
                            music_note = msg;
                            next_music = Instant::now() + Duration::from_millis(800);
                        }
                    }
                }
                Update::Notice(text) => status = text,
                Update::NetDetails(details) => net_details = details,
                Update::AudioCheck => next_audio = Instant::now(),
                Update::Network(state) => {
                    net = state;
                    if network::portal_offer(&mut portal_offered, state)
                        && unlocked
                        && confirm.is_none()
                    {
                        confirm = Some(Confirm {
                            label: "This Wi-Fi needs a login page. Open the text login now?".into(),
                            body: json!({"kind":"portal"}),
                        });
                    } else if state == "No Wi-Fi" {
                        status = "No Wi-Fi | Esc, then W for Wi-Fi".into();
                    }
                }
                Update::Link(state, reachable, gateway, headset) => {
                    vpn_state = state;
                    bt_connected = headset;
                    if vpn::should_learn_home(
                        state,
                        reachable,
                        home_id.as_deref(),
                        gateway.as_deref(),
                    ) {
                        if let Some(id) = &gateway {
                            vpn::save_home(id);
                            home_id = Some(id.clone());
                        }
                    }
                    let home = vpn::home_match(home_id.as_deref(), gateway.as_deref());
                    let bt_wanted = bluetooth::bluetooth_wanted(home, net);
                    // A power glitch resets the USB adapter: every third check (about 30
                    // seconds) at home, switch back on any adapter that dropped out.
                    bt_keep_ticks = bt_keep_ticks.wrapping_add(1);
                    if bt_keep_ticks % 3 == 0
                        && bt_wanted == Some(true)
                        && !bt_off_by_hand
                        && !bt_busy.load(std::sync::atomic::Ordering::Relaxed)
                    {
                        tasks.push(tokio::task::spawn_blocking(|| {
                            if bluetooth::needs_power_on(
                                Some(true),
                                false,
                                &bluetooth::adapter_power(),
                            ) {
                                bluetooth::set_power(true);
                            }
                            bluetooth::restore_audio();
                        }));
                    }
                    if let Some(on) = bt_policy.observe(bt_wanted) {
                        bt_off_by_hand = false;
                        status = if on {
                            "Home network | Bluetooth on".into()
                        } else {
                            "Away or offline | Bluetooth off".into()
                        };
                        tasks.push(tokio::task::spawn_blocking(move || {
                            bluetooth::set_power(on)
                        }));
                    }
                    unreachable_ticks = if reachable {
                        0
                    } else {
                        unreachable_ticks.saturating_add(1)
                    };
                    let facts = vpn::Facts {
                        vpn: state,
                        home: vpn::home_match(home_id.as_deref(), gateway.as_deref()),
                        internet: net == "Internet available",
                        node_reachable: reachable,
                        unreachable_ticks,
                        manual_hold: vpn_hold_until.is_some_and(|t| Instant::now() < t),
                    };
                    if vpn::auto_enabled() {
                        match vpn::decide(&facts) {
                            vpn::Action::Connect => {
                                unreachable_ticks = 0;
                                status = "Away from home | connecting VPN".into();
                                tasks.push(tokio::task::spawn_blocking(vpn::connect));
                            }
                            vpn::Action::Disconnect => {
                                status = "Home network | dropping VPN".into();
                                tasks.push(tokio::task::spawn_blocking(vpn::disconnect));
                            }
                            vpn::Action::Nothing => {}
                        }
                    }
                }
                Update::Read(g, p, result) if g == epoch => {
                    reading = false;
                    match result {
                        Ok(value) => {
                            data[p] = value;
                            if !items(&data[p]).is_empty() {
                                selected[p] =
                                    selected[p].min(items(&data[p]).len().saturating_sub(1));
                            }
                            last_read = Instant::now();
                            failures = 0;
                            status = if unlocked {
                                "Home connected"
                            } else {
                                "Home connected | Locked"
                            }
                            .into();
                        }
                        Err(error) => {
                            failures = failures.saturating_add(1);
                            status = error;
                        }
                    }
                    next_read = Instant::now()
                        + Duration::from_secs(if failures == 0 {
                            30
                        } else {
                            2u64.saturating_pow(failures.min(5)).min(30)
                        });
                    if p != page {
                        next_read = Instant::now();
                    }
                }
                Update::Action(g, result) if g == epoch => {
                    busy = false;
                    match result {
                        Ok(value) => {
                            status =
                                format!("{} {}", line(&value["state"]), line(&value["summary"]));
                            if value["state"] != "uncertain" {
                                let _ = endora_terminal::storage::Paths::state()?
                                    .remove("pending-operation");
                            }
                        }
                        Err(_) => {
                            status =
                                "Outcome uncertain; V checks the operation before any resubmission"
                                    .into()
                        }
                    }
                    next_read = Instant::now();
                }
                _ => {}
            }
            dirty = true;
        }
        let desired = if stage == Stage::Pocket {
            Stage::Pocket
        } else {
            power.stage(last_input.elapsed(), busy)
        };
        if desired != stage {
            if power::backlight(desired) {
                stage = desired;
            } else {
                status = "Backlight support needs device-install.sh".into();
                stage = desired;
            }
            if stage == Stage::Pocket {
                power.pocket();
                terminal
                    .clear()
                    .map_err(|_| "Cannot blank private screen")?;
                pocket_since = Some(Instant::now());
                last_input = Instant::now() - Duration::from_secs(300);
            }
            dirty = true;
        }
        if page == 8
            && unlocked
            && stage != Stage::Pocket
            && Instant::now() >= next_bt
            && !bt_busy.load(std::sync::atomic::Ordering::Relaxed)
        {
            next_bt = Instant::now() + Duration::from_secs(3);
            let tx = tx.clone();
            tasks.push(tokio::task::spawn_blocking(move || {
                let _ = tx.blocking_send(Update::Bt(bt_snapshot()));
            }));
        }
        if unlocked
            && ((page == 7 && stage != Stage::Pocket) || radio.is_some())
            && Instant::now() >= next_music
        {
            next_music = Instant::now() + Duration::from_secs(5);
            let (tx, player, node) = (tx.clone(), player.clone(), client.clone());
            tasks.push(tokio::spawn(async move {
                let _ = tx
                    .send(Update::Music(
                        music_work(player, node, MusicAction::Poll).await,
                    ))
                    .await;
            }));
        }
        if Instant::now() >= next_audio {
            next_audio = Instant::now() + Duration::from_secs(60);
            if !audio_busy.swap(true, std::sync::atomic::Ordering::Relaxed) {
                let (state, busy, tx) = (audio_state.clone(), audio_busy.clone(), tx.clone());
                tasks.push(tokio::task::spawn_blocking(move || {
                    let outcome = state.lock().ok().and_then(|mut s| {
                        let (guard, id) = &mut *s;
                        endora_terminal::audio::guard_tick(guard, id)
                    });
                    busy.store(false, std::sync::atomic::Ordering::Relaxed);
                    if let Some((note, look_again)) = outcome {
                        if let Some(note) = note {
                            let _ = tx.blocking_send(Update::Notice(note));
                        }
                        if look_again {
                            // Whether the fix worked is checked a few seconds later.
                            std::thread::sleep(Duration::from_secs(5));
                            let _ = tx.blocking_send(Update::AudioCheck);
                        }
                    }
                }));
            }
        }
        if stage != Stage::Pocket && Instant::now() >= next_net {
            let tx = tx.clone();
            let node_url = settings.config().map(|c| c.url).unwrap_or_default();
            tasks.push(tokio::spawn(async move {
                let state = tokio::task::spawn_blocking(network::state)
                    .await
                    .unwrap_or("NetworkManager unavailable");
                let _ = tx.send(Update::Network(state)).await;
                let link = tokio::task::spawn_blocking(move || {
                    (
                        vpn::status(),
                        vpn::node_reachable(&node_url),
                        vpn::gateway_id(),
                        !bluetooth::connected().is_empty(),
                    )
                })
                .await;
                if let Ok((state, reachable, gateway, headset)) = link {
                    let _ = tx
                        .send(Update::Link(state, reachable, gateway, headset))
                        .await;
                }
            }));
            next_net = Instant::now() + Duration::from_secs(10);
        }
        if stage != Stage::Pocket
            && !busy
            && !reading
            && page < 6
            && unlocked
            && Instant::now() >= next_read
        {
            if let Some(task) = read(
                &client,
                page,
                &search,
                epoch,
                home_catalog,
                detail
                    .as_ref()
                    .filter(|(p, _)| *p == page)
                    .map(|(_, path)| path.as_str()),
                &tx,
            ) {
                tasks.push(task);
                reading = true;
            }
            next_read = Instant::now() + Duration::from_secs(30);
        }
        if Instant::now() >= next_sample {
            let sample = telemetry::take_sample(
                match stage {
                    Stage::Awake => "awake",
                    Stage::Dim => "dim",
                    Stage::Pocket => "pocket",
                },
                if power.mode == Mode::Desk {
                    "desk"
                } else {
                    "travel"
                },
                page as u8,
            );
            if let Some(h) = &power_log {
                let _ = h.append(&sample);
            }
            battery = sample.bat.map(|pct| telemetry::Battery {
                pct,
                charging: sample.charging == Some(true),
            });
            recent.push(sample);
            let cutoff = now_secs().saturating_sub(3 * 3600);
            recent.retain(|s| s.t >= cutoff);
            minutes_left = telemetry::estimate_minutes_left(&recent, now_secs());
            next_sample = Instant::now() + Duration::from_secs(60);
            dirty = true;
        }
        if dirty && stage != Stage::Pocket {
            // The menu opens with its cursor on the page you are on.
            if menu && !menu_shown {
                use endora_terminal::nav::{MENU, Target};
                menu_sel = MENU
                    .iter()
                    .position(|m| match m.target {
                        Target::Page(p) => p == page,
                        Target::Settings(_) => page == 6,
                        _ => false,
                    })
                    .unwrap_or(0);
            }
            menu_shown = menu;
            let settings_status = endora_terminal::nav::SettingsStatus {
                wifi: net.to_owned(),
                bluetooth: if !bt_conn.is_empty() {
                    "Connected".into()
                } else if bt_powered {
                    "On".into()
                } else {
                    String::new()
                },
                text_size: endora_terminal::font::label(text_size).into(),
                screen: if power.mode == Mode::Desk {
                    "Desk"
                } else {
                    "Travel"
                }
                .into(),
                profile: if settings.away { "Away" } else { "Home" }.into(),
                vpn: format!("{vpn_state:?}"),
                version: env!("CARGO_PKG_VERSION").into(),
                start: start_screen.label().into(),
                network: endora_terminal::network::net_lines(
                    &net_details,
                    &format!("{vpn_state:?}"),
                    vpn_hold_until
                        .and_then(|t| t.checked_duration_since(Instant::now()))
                        .map(|left| left.as_secs() / 60 + 1),
                ),
            };
            // Remember the page in use, for a "Last used" start screen.
            if unlocked && saved_page != Some(page) {
                saved_page = Some(page);
                let _ = endora_terminal::storage::Paths::state()
                    .and_then(|p| p.write("last-page", page.to_string().as_bytes()));
            }
            terminal.draw(|f|{
                let compose=!menu && confirm.is_none() && unlocked && page==3;
                let areas=Layout::vertical([Constraint::Length(1),Constraint::Min(3),Constraint::Length(if compose{3}else{0}),Constraint::Length(1),Constraint::Length(1)]).split(f.area());
                f.render_widget(Paragraph::new(format!("Endora / {} / {}",if menu{"Menu"}else{NAMES[page]},if unlocked{"Unlocked"}else{"Locked"})),areas[0]);
                f.render_widget(Paragraph::new(status_right(&vpn::status_icons(net,vpn_state,bt_connected),&local_time(),&telemetry::battery_text(battery,minutes_left))).alignment(ratatui::layout::Alignment::Right),areas[0]);
                let mut body=if let Some(confirm)=&confirm {format!("{}\n\nY confirm | Back cancel",confirm.label)}else if menu {endora_terminal::nav::menu_view(menu_sel)}else if page==6 {endora_terminal::nav::settings_view(settings_section,settings_sel,&settings_status)}else if page==8 && unlocked {endora_terminal::presentation::bluetooth_view(bt_powered,&bt_conn,&bt_paired,&bt_found,bt_window,selected[8],&bt_note)}else if page==7 && unlocked {endora_terminal::presentation::music_view(now_playing.as_ref(),&music_title,&music_items,selected[7],spotify::is_signed_in(),&music_note,radio.is_some()&&is_radio_title(&music_title))}else if !unlocked && page!=0 {"Unlock to use this page. Press U or open the menu.".into()}else{display(&data[page],selected[page])};
                if page==0 && unlocked && !menu && confirm.is_none() && detail.as_ref().is_none_or(|(p,_)|*p!=0) {dash_width=areas[1].width.saturating_sub(2) as usize;let (text,at)=home(&data[0],"",selected[0],dash_width);body=text;let rows=areas[1].height.saturating_sub(2) as usize;let top=scroll[0] as usize;if at<top{scroll[0]=at as u16;}else if rows>0&&at>=top+rows{scroll[0]=(at+1-rows) as u16;}} else if page==0 && unlocked && !menu && confirm.is_none() {body=display(&data[0],0);}
                if !unlocked && prompt.is_some() {body=String::new();}
                if page==0 && unlocked && !menu && confirm.is_none() && detail.as_ref().is_none() {body.push_str(&format!("\n{net}\nUpdated {}s ago",last_read.elapsed().as_secs()));}
                if page==6 && !menu {if let Some(seconds)=usage.suggestion(){body.push_str(&format!("\nSuggested Travel screen-off: {seconds}s\nA accepts; current: {}s",usage.pocket_seconds));}}
                let count=Paragraph::new(body.as_str()).wrap(Wrap{trim:false}).line_count(areas[1].width.saturating_sub(2).max(1));bottom=count.saturating_sub(areas[1].height.saturating_sub(2) as usize).min(u16::MAX as usize) as u16;
                scroll[page]=scroll[page].min(bottom);
                f.render_widget(Paragraph::new(body).wrap(Wrap{trim:false}).scroll((if menu||confirm.is_some(){0}else{scroll[page]},0)).block(Block::default().borders(Borders::ALL)),areas[1]);
                if compose{composer::draw(f,areas[2],&search,cursor.min(search.len()));}
                let hint=match page{8=>"P pair | S speaker | X forget | Enter connect | Esc back",7=>"Up/Down + Enter | Space play or pause | Esc back",0=>"Arrows move | Enter or a key opens | Esc menu",2=>"C catalog | Space favorite | O on | F off | A scene | Esc menu",3=>"Type + Enter search | Up/Down pick | Enter request | Esc menu",4=>"Up/Down select | I logs | S start | T stop | R restart | Esc menu",5=>"Enter run | I logs | V reconcile | Esc menu",6=>"Up/Down + Enter, or a letter | Esc back",_=>"Esc menu"};
                f.render_widget(Paragraph::new(hint),areas[3]);f.render_widget(Paragraph::new(clean(&status)),areas[4]);
                if let (false,Some(p))=(unlocked,&prompt){
                    let r=endora_terminal::presentation::lock_box(f.area());
                    f.render_widget(ratatui::widgets::Clear,r);
                    let dots=endora_terminal::presentation::passcode_line(p,reveal_passcode);
                    f.render_widget(Paragraph::new(format!("Endora\nPasscode\n{dots}\n{}\nEnter unlock | Esc Wi-Fi\nClick or Tab shows it",endora_terminal::presentation::lock_notice(caps_on,endora_terminal::presentation::lock_hint(net)))).alignment(ratatui::layout::Alignment::Center).block(Block::default().borders(Borders::ALL)),r);
                }
            }).map_err(|_|"Cannot draw screen")?;
            dirty = false;
        }
        // Caps lock makes no key event, so its light is read (twice a second) while the box is up.
        if !unlocked && prompt.is_some() && caps_checked.elapsed() >= Duration::from_millis(400) {
            caps_checked = Instant::now();
            let now = endora_terminal::keyboard::caps_lock_on();
            if now != caps_on {
                caps_on = now;
                dirty = true;
            }
        }
        let delta = wheel.poll();
        // Volume keys: steps pile up in the wheel and are applied about eight times a second at
        // most, so a held key is smooth and a burst is not lost.
        if last_volume.elapsed() >= Duration::from_millis(120) {
            let volume_steps = wheel.take_volume();
            if volume_steps != 0 {
                last_volume = Instant::now();
                let tx = tx.clone();
                tasks.push(tokio::task::spawn_blocking(move || {
                    use endora_terminal::keyboard::{parse_volume, volume_command};
                    let (program, args) = volume_command(volume_steps);
                    let _ = std::process::Command::new(program).args(args).status();
                    let level = std::process::Command::new("wpctl")
                        .args(["get-volume", "@DEFAULT_AUDIO_SINK@"])
                        .output()
                        .ok()
                        .and_then(|o| parse_volume(&String::from_utf8_lossy(&o.stdout)));
                    if let Some(level) = level {
                        let _ = tx.blocking_send(Update::Notice(format!("Volume {level}%")));
                    }
                }));
            }
        }
        // Holding the hang-up key restarts the device; a countdown shows after the first second.
        match wheel.right_held() {
            Some(held) if held >= endora_terminal::wheel::RESTART_HOLD => {
                if !restarting {
                    restarting = true;
                    status = "Restarting the device...".into();
                    dirty = true;
                    let _ = power::reboot();
                }
            }
            Some(held) if held >= Duration::from_secs(1) && !restarting => {
                let left = endora_terminal::wheel::RESTART_HOLD
                    .saturating_sub(held)
                    .as_secs()
                    + 1;
                let text = format!("Keep holding the hang-up key to restart ({left})...");
                if status != text {
                    status = text;
                    dirty = true;
                }
            }
            None if !restarting && status.starts_with("Keep holding the hang-up key") => {
                status.clear();
                dirty = true;
            }
            _ => {}
        }
        let sideways = wheel.take_sideways();
        // On the passcode box a trackpad click shows or hides what was typed; everywhere else
        // the trackpad is the arrow keys and its click is Enter (not while the screen is off).
        let clicked = wheel.take_click();
        if !unlocked && prompt.is_some() {
            if clicked {
                reveal_passcode = !reveal_passcode;
                dirty = true;
            }
        } else if stage != Stage::Pocket {
            pending_keys.extend(endora_terminal::nav::trackpad_keys(delta, clicked));
        }
        if sideways != 0
            && page == 0
            && unlocked
            && !menu
            && confirm.is_none()
            && detail.as_ref().is_none_or(|(p, _)| *p != 0)
        {
            let rows = dash_rows(&data[0], dash_width);
            for _ in 0..sideways.unsigned_abs().min(4) {
                selected[0] = dash_move(
                    &rows,
                    selected[0],
                    if sideways > 0 {
                        Move::Right
                    } else {
                        Move::Left
                    },
                );
            }
            dirty = true;
        }
        let event = if event::poll(Duration::from_millis(50)).map_err(|_| "Cannot read keyboard")? {
            Some(event::read().map_err(|_| "Cannot read keyboard")?)
        } else {
            None
        };
        if wheel.take_activity() || matches!(event, Some(Event::Key(_) | Event::Mouse(_))) {
            if stage == Stage::Pocket {
                if let Some(since) = pocket_since.take() {
                    usage.record(since.elapsed());
                    let _ = usage.save();
                }
                power.wake();
                let _ = power::backlight(Stage::Awake);
                stage = Stage::Awake;
                last_input = Instant::now();
                dirty = true;
                continue;
            }
            last_input = Instant::now();
            if stage == Stage::Dim {
                let _ = power::backlight(Stage::Awake);
                stage = Stage::Awake;
            }
            dirty = true;
        }
        let event = event.or_else(|| {
            pending_keys
                .pop_front()
                .map(|code| Event::Key(event::KeyEvent::new(code, KeyModifiers::NONE)))
        });
        let Some(event) = event else {
            tokio::task::yield_now().await;
            continue;
        };
        let key = match event {
            Event::Resize(..) => {
                dirty = true;
                continue;
            }
            Event::Mouse(mouse) => {
                let delta = match mouse.kind {
                    MouseEventKind::ScrollUp => 1,
                    MouseEventKind::ScrollDown => -1,
                    _ => 0,
                };
                // The trackpad scrolls: up shows the passcode being typed, down hides it.
                if !unlocked && prompt.is_some() {
                    if delta != 0 {
                        reveal_passcode = delta > 0;
                        dirty = true;
                    }
                    continue;
                }
                // Elsewhere a scroll is an arrow key, so every screen moves its cursor.
                pending_keys.extend(endora_terminal::nav::trackpad_keys(delta, false));
                continue;
            }
            Event::Key(key) if key.kind == KeyEventKind::Press => key,
            _ => continue,
        };
        if key.code == KeyCode::Char('q') && key.modifiers.contains(KeyModifiers::CONTROL) {
            break;
        }
        if !unlocked && prompt.is_some() {
            // Raw keys: a passcode must not be case-folded like a shortcut.
            match key.code {
                KeyCode::Tab => reveal_passcode = !reveal_passcode,
                // Ctrl+C (the Q20's Alt key sends Ctrl): turn caps lock off.
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    if endora_terminal::keyboard::clear_caps_lock() {
                        caps_checked = Instant::now() - Duration::from_secs(1);
                    }
                }
                KeyCode::Up => reveal_passcode = true,
                KeyCode::Down => reveal_passcode = false,
                KeyCode::Enter => {
                    reveal_passcode = false;
                    let pass = prompt.take().unwrap_or_default();
                    match endora_terminal::portable::unlock(&settings, &pass) {
                        Ok(c) => {
                            client = Some(c);
                            unlocked = true;
                            let (start, open_menu) =
                                endora_terminal::nav::start_view(start_screen, saved_page);
                            page = start;
                            menu = open_menu;
                            status = "Unlocked".into();
                            next_read = Instant::now();
                        }
                        Err(error) => {
                            status = error;
                            prompt = Some(zeroize::Zeroizing::new(String::new()));
                        }
                    }
                }
                KeyCode::Backspace => {
                    if let Some(p) = prompt.as_mut() {
                        p.pop();
                    }
                }
                KeyCode::Char(c)
                    if !key.modifiers.contains(KeyModifiers::CONTROL) && !c.is_control() =>
                {
                    if let Some(p) = prompt.as_mut().filter(|p| p.len() < 128) {
                        p.push(c);
                    }
                }
                KeyCode::Esc => {
                    reveal_passcode = false;
                    prompt = None;
                    page = 6;
                    settings_section = Some(endora_terminal::nav::Section::WiFi);
                    settings_sel = 0;
                    next_read = Instant::now();
                }
                _ => {}
            }
            dirty = true;
            continue;
        }
        let mut code = endora_terminal::protocol::shortcut(
            key.code,
            !unlocked || menu || page != 3 || key.modifiers.contains(KeyModifiers::CONTROL),
        );
        if code == KeyCode::Char('q') && key.modifiers.contains(KeyModifiers::CONTROL) {
            break;
        }
        if confirm.is_some() {
            if code == KeyCode::Esc || matches!(code, KeyCode::Char('n' | 'N')) {
                confirm = None;
                continue;
            }
            if matches!(code, KeyCode::Char('y' | 'Y')) {
                let pending = confirm.take().unwrap();
                if pending.body["kind"] == "btforget" {
                    let (tx, busy) = (tx.clone(), bt_busy.clone());
                    bt_note = "Forgetting all pairings...".into();
                    tasks.push(tokio::task::spawn_blocking(move || {
                        bt_task(tx, busy, || {
                            Some(format!("Forgot {} pairing(s).", bluetooth::forget_all()))
                        })
                    }));
                    continue;
                }
                if pending.body["kind"] == "portal" {
                    wheel.close();
                    drop(guard.take());
                    let result = super::text_browser("portal", "http://neverssl.com/");
                    guard = Some(Screen::enter()?);
                    wheel = ConsoleWheel::open();
                    terminal.clear().map_err(|_| "Cannot restore screen")?;
                    status = result.err().unwrap_or("Returned from Wi-Fi login".into());
                    last_input = Instant::now();
                    next_net = Instant::now();
                    continue;
                }
                if pending.body["kind"] == "reboot" {
                    status = "Restarting the device...".into();
                    let _ = power::reboot();
                    continue;
                }
                if pending.body["kind"] == "shutdown" {
                    drop(guard.take());
                    let _ = Command::new("systemctl").arg("poweroff").status();
                    guard = Some(Screen::enter()?);
                    continue;
                }
                if let Some(c) = client.clone() {
                    let tx = tx.clone();
                    let g = epoch;
                    busy = true;
                    endora_terminal::storage::Paths::state()?.write("pending-operation",serde_json::to_string(&json!({"node":settings.config()?.url,"request_id":pending.body["request_id"]})).map_err(|_|"Cannot save operation identifier")?.as_bytes())?;
                    tasks.push(tokio::spawn(async move {
                        let result = c.post("/v1/terminal/action", &pending.body).await;
                        let _ = tx.send(Update::Action(g, result)).await;
                    }));
                }
            }
            continue;
        }
        if (!unlocked || menu || page != 3) && code == KeyCode::Char('u')
            || (code == KeyCode::Char('u') && key.modifiers.contains(KeyModifiers::CONTROL))
        {
            if !unlocked {
                prompt = Some(zeroize::Zeroizing::new(String::new()));
                page = 0;
                dirty = true;
            }
            continue;
        }
        if menu {
            use endora_terminal::nav::{MENU, Target, menu_key};
            let chosen = match code {
                KeyCode::Esc => {
                    menu = false;
                    None
                }
                KeyCode::Up => {
                    menu_sel = menu_sel.saturating_sub(1);
                    None
                }
                KeyCode::Down => {
                    menu_sel = (menu_sel + 1).min(MENU.len() - 1);
                    None
                }
                KeyCode::Enter => Some(menu_sel),
                KeyCode::Char(c) => menu_key(c),
                _ => None,
            };
            let Some(index) = chosen else {
                continue;
            };
            menu = false;
            back_page = None;
            match MENU[index].target {
                Target::Page(index) => {
                    page = index;
                    next_read = Instant::now();
                    next_bt = Instant::now();
                    cursor = search.len();
                    continue;
                }
                Target::Settings(section) => {
                    page = 6;
                    settings_section = section;
                    settings_sel = 0;
                    continue;
                }
                Target::Pocket => {
                    terminal
                        .clear()
                        .map_err(|_| "Cannot blank private screen")?;
                    pocket_since = Some(Instant::now());
                    power.pocket();
                    let _ = power::backlight(Stage::Pocket);
                    stage = Stage::Pocket;
                    last_input = Instant::now() - Duration::from_secs(300);
                    continue;
                }
                Target::Lock => {
                    // Locking is the K action below; it needs a page where K is not typed text.
                    page = 0;
                    code = KeyCode::Char('k');
                }
            }
        }
        // Music page: Y signs in to Spotify when it is not set up (it runs the Settings flow and
        // comes back here).
        if page == 7 && unlocked && !busy && code == KeyCode::Char('y') && !spotify::is_signed_in()
        {
            flow_return = Some(7);
            page = 6;
            settings_section = None;
        }
        // Music menu: Enter runs the highlighted choice, exactly as its key would.
        if page == 7
            && unlocked
            && code == KeyCode::Enter
            && music_items.is_empty()
            && spotify::is_signed_in()
        {
            if let Some((key, _)) = endora_terminal::presentation::MUSIC_MENU.get(selected[7]) {
                code = KeyCode::Char(*key);
            }
        }
        // Settings: arrows, Enter, letters and Esc move through the sections; actions reuse the
        // device actions below by their original key.
        if page == 6 && flow_return.is_none() {
            use endora_terminal::nav::{Action, settings_key};
            match settings_key(settings_section, settings_sel, code) {
                Some(Action::Select(i)) => {
                    settings_sel = i;
                    continue;
                }
                Some(Action::Back) => {
                    if settings_section.take().is_some() {
                        settings_sel = 0;
                    } else {
                        menu = true;
                    }
                    continue;
                }
                Some(Action::Open(section)) => {
                    settings_section = Some(section);
                    settings_sel = 0;
                    if section == endora_terminal::nav::Section::Network {
                        let tx = tx.clone();
                        tasks.push(tokio::task::spawn_blocking(move || {
                            let _ = tx.blocking_send(Update::NetDetails(
                                endora_terminal::network::details(),
                            ));
                        }));
                    }
                    continue;
                }
                Some(Action::NetRefresh) => {
                    status = "Reading network details...".into();
                    next_net = Instant::now();
                    let tx = tx.clone();
                    tasks.push(tokio::task::spawn_blocking(move || {
                        let _ = tx
                            .blocking_send(Update::NetDetails(endora_terminal::network::details()));
                        let _ = tx.blocking_send(Update::Notice("Network details updated".into()));
                    }));
                    continue;
                }
                Some(Action::Vpn(on)) => {
                    // A manual choice holds for 15 minutes before automatic switching resumes.
                    vpn_hold_until = Some(Instant::now() + Duration::from_secs(15 * 60));
                    status = if on {
                        "Connecting the VPN..."
                    } else {
                        "Disconnecting the VPN..."
                    }
                    .into();
                    let tx = tx.clone();
                    tasks.push(tokio::task::spawn_blocking(move || {
                        if on {
                            vpn::connect();
                        } else {
                            vpn::disconnect();
                        }
                        std::thread::sleep(Duration::from_secs(3));
                        let now = vpn::status();
                        let _ = tx.blocking_send(Update::Notice(format!("VPN: {now:?}")));
                        let _ = tx
                            .blocking_send(Update::NetDetails(endora_terminal::network::details()));
                    }));
                    next_net = Instant::now() + Duration::from_secs(4);
                    continue;
                }
                Some(Action::VpnTest) => {
                    status = "Testing the VPN...".into();
                    let tx = tx.clone();
                    let node_url = settings.config().map(|c| c.url).unwrap_or_default();
                    tasks.push(tokio::task::spawn_blocking(move || {
                        let tunnel = vpn::status();
                        let trace = vpn::trace();
                        let node = vpn::node_reachable(&node_url);
                        let through = match trace.as_ref().and_then(|t| t.warp.as_deref()) {
                            Some("on") | Some("plus") => "traffic goes through WARP",
                            Some(_) => "traffic does not go through WARP",
                            None => "could not reach Cloudflare",
                        };
                        let ip = trace
                            .and_then(|t| t.ip)
                            .map_or(String::new(), |ip| format!(" | public IP {ip}"));
                        let node = if node {
                            "node reachable"
                        } else {
                            "node not reachable"
                        };
                        let _ = tx.blocking_send(Update::Notice(format!(
                            "VPN {tunnel:?}: {through}{ip} | {node}"
                        )));
                    }));
                    continue;
                }
                Some(Action::Page(index)) => {
                    back_page = Some(6);
                    page = index;
                    next_bt = Instant::now();
                    continue;
                }
                Some(Action::Legacy(c)) => code = KeyCode::Char(c),
                Some(Action::TextSize(step)) => {
                    code = KeyCode::Char(if step > 0 { '+' } else { '-' })
                }
                Some(Action::Restart) => {
                    confirm = Some(Confirm {
                        label: "Restart the device?".into(),
                        body: json!({"kind":"reboot"}),
                    });
                    continue;
                }
                Some(Action::StartScreen) => {
                    start_screen = start_screen.next();
                    let _ = endora_terminal::storage::Paths::state()
                        .and_then(|p| p.write("start-screen", start_screen.as_str().as_bytes()));
                    status = format!("Start screen: {}", start_screen.label());
                    continue;
                }
                Some(Action::SoundTest) => {
                    status = "Sound test: left, then right, then both".into();
                    let tx = tx.clone();
                    tasks.push(tokio::task::spawn_blocking(move || {
                        for (name, left, right) in endora_terminal::audio::STEPS {
                            let _ = tx.blocking_send(Update::Notice(format!(
                                "Sound test: playing the {name}..."
                            )));
                            if let Err(e) = endora_terminal::audio::play(left, right) {
                                let _ = tx.blocking_send(Update::Notice(e.to_string()));
                                return;
                            }
                        }
                        let _ = tx.blocking_send(Update::Notice("Sound test done".into()));
                    }));
                    continue;
                }
                // Text size still works anywhere; other unknown letters do nothing here.
                None if matches!(code, KeyCode::Char('+' | '-' | '=') | KeyCode::Tab) => {}
                None => continue,
            }
        }
        // Home screen: a service's own key selects it and opens it, like Enter on the cursor.
        if page == 0
            && unlocked
            && !busy
            && detail.as_ref().is_none_or(|(p, _)| *p != 0)
            && !key.modifiers.contains(KeyModifiers::CONTROL)
        {
            if let KeyCode::Char(c) = code {
                if let Some(index) =
                    dash_key_index(c).filter(|i| *i < dash_services(&data[0]).len())
                {
                    selected[0] = index;
                    code = KeyCode::Enter;
                }
            }
        }
        match code {
            // + and - change the text size anywhere except where text is typed (search) and the Music
            // page, where they are the volume keys.
            KeyCode::Char(c @ ('+' | '=' | '-'))
                if !key.modifiers.contains(KeyModifiers::CONTROL)
                    && (!unlocked || !matches!(page, 3 | 7)) =>
            {
                use endora_terminal::font;
                let next = font::step(text_size, if c == '-' { -1 } else { 1 });
                status = if next == text_size {
                    format!("Text size: {} (the limit)", font::label(text_size))
                } else {
                    match font::apply(next) {
                        Ok(()) => {
                            text_size = next;
                            font::save(next);
                            format!("Text size: {} | + larger, - smaller", font::label(next))
                        }
                        Err(error) => error,
                    }
                };
                let _ = terminal.clear();
                dirty = true;
            }
            KeyCode::Esc if page == 8 && bt_window.is_some() => {
                bt_cancel.store(true, std::sync::atomic::Ordering::Relaxed);
                bt_note = "Closing the pairing window...".into();
            }
            KeyCode::Esc if page == 8 && !bt_found.is_empty() => bt_found.clear(),
            KeyCode::Esc
                if page == 7 && unlocked && music_prev.is_none() && !music_items.is_empty() =>
            {
                music_items.clear();
                music_title.clear();
                selected[7] = 0;
                music_note.clear();
            }
            KeyCode::Esc if page == 7 && unlocked && music_prev.is_some() => {
                if let Some((title, items)) = music_prev.take() {
                    music_title = title;
                    music_items = items;
                    selected[7] = 0;
                    music_note.clear();
                }
            }
            KeyCode::Esc => {
                if detail.as_ref().is_some_and(|(p, _)| *p == page) {
                    detail = None;
                    next_read = Instant::now();
                } else if let Some(previous) = back_page.take() {
                    page = previous;
                } else {
                    menu = true;
                }
            }
            KeyCode::Enter
                if page == 0
                    && unlocked
                    && !busy
                    && detail.as_ref().is_none_or(|(p, _)| *p != 0) =>
            {
                // Open the selected service's container log (read-only).
                if dash_services(&data[0])
                    .get(selected[0])
                    .is_some_and(|s| s["id"].as_str().is_none())
                {
                    status = "No container behind this one".into();
                } else if let (Some(c), Some(id)) = (
                    client.clone(),
                    dash_services(&data[0])
                        .get(selected[0])
                        .and_then(|s| s["id"].as_str())
                        .map(str::to_owned),
                ) {
                    let path = format!("/v1/terminal/logs/{id}");
                    detail = Some((0, path.clone()));
                    scroll[0] = 0;
                    let tx = tx.clone();
                    let g = epoch;
                    tasks.push(tokio::spawn(async move {
                        let _ = tx.send(Update::Read(g, 0, c.get(&path).await)).await;
                    }));
                }
            }
            KeyCode::Enter if page == 0 || page == 6 => menu = true,
            KeyCode::Char('k') if page != 3 && unlocked => {
                for task in tasks.drain(..) {
                    task.abort();
                    let _ = task.await;
                }
                epoch += 1;
                unlocked = false;
                client = portable::locked(&settings).ok();
                // The music session holds the unlocked client; it goes with it.
                *player.lock().await = None;
                reading = false;
                busy = false;
                confirm = None;
                page = 0;
                for d in data.iter_mut() {
                    *d = json!({});
                }
                prompt = Some(zeroize::Zeroizing::new(String::new()));
                status = "Locked".into();
            }
            KeyCode::Char('p') if !matches!(page, 3 | 6 | 8) => {
                terminal
                    .clear()
                    .map_err(|_| "Cannot blank private screen")?;
                pocket_since = Some(Instant::now());
                power.pocket();
                let _ = power::backlight(Stage::Pocket);
                stage = Stage::Pocket;
                last_input = Instant::now() - Duration::from_secs(300);
            }
            KeyCode::Tab => {
                page = endora_terminal::nav::next_page(page);
                next_read = Instant::now();
                cursor = search.len();
            }
            KeyCode::Up | KeyCode::Down
                if page == 0 && unlocked && detail.as_ref().is_none_or(|(p, _)| *p != 0) =>
            {
                let rows = dash_rows(&data[0], dash_width);
                selected[0] = dash_move(
                    &rows,
                    selected[0],
                    if code == KeyCode::Up {
                        Move::Up
                    } else {
                        Move::Down
                    },
                );
            }
            KeyCode::Left | KeyCode::Right
                if page == 0 && unlocked && detail.as_ref().is_none_or(|(p, _)| *p != 0) =>
            {
                let rows = dash_rows(&data[0], dash_width);
                selected[0] = dash_move(
                    &rows,
                    selected[0],
                    if code == KeyCode::Left {
                        Move::Left
                    } else {
                        Move::Right
                    },
                );
            }
            KeyCode::Up | KeyCode::Down if page == 8 && unlocked => {
                let len = if bt_found.is_empty() {
                    bt_paired.len()
                } else {
                    bt_found.len()
                };
                if len > 0 {
                    selected[8] = if code == KeyCode::Up {
                        selected[8].saturating_sub(1)
                    } else {
                        (selected[8] + 1).min(len - 1)
                    };
                }
            }
            KeyCode::Enter
                if page == 8 && unlocked && !bt_busy.load(std::sync::atomic::Ordering::Relaxed) =>
            {
                let (tx, busy) = (tx.clone(), bt_busy.clone());
                if let Some(device) = bt_found.get(selected[8]).cloned() {
                    bt_found.clear();
                    bt_note = format!("Pairing with {}...", device.name);
                    tasks.push(tokio::task::spawn_blocking(move || {
                        bt_task(tx, busy, move || {
                            Some(match bluetooth::pair_and_connect(&device) {
                                Ok(()) => format!("Connected to {}.", device.name),
                                Err(e) => e,
                            })
                        })
                    }));
                } else if let Some(device) = bt_paired.get(selected[8]).cloned() {
                    let was_on = bt_conn
                        .iter()
                        .any(|c| c.mac.eq_ignore_ascii_case(&device.mac));
                    bt_note = format!(
                        "{} {}...",
                        if was_on {
                            "Disconnecting"
                        } else {
                            "Connecting to"
                        },
                        device.name
                    );
                    tasks.push(tokio::task::spawn_blocking(move || {
                        bt_task(tx, busy, move || {
                            let result = if was_on {
                                bluetooth::disconnect(&device)
                            } else {
                                bluetooth::connect(&device)
                            };
                            Some(match result {
                                Ok(()) => format!(
                                    "{} {}.",
                                    if was_on {
                                        "Disconnected"
                                    } else {
                                        "Connected to"
                                    },
                                    device.name
                                ),
                                Err(e) => e,
                            })
                        })
                    }));
                }
            }
            KeyCode::Char(c @ ('p' | 's' | 'f' | 'r' | 't' | 'x' | 'a'))
                if page == 8 && unlocked && !bt_busy.load(std::sync::atomic::Ordering::Relaxed) =>
            {
                let (tx, busy) = (tx.clone(), bt_busy.clone());
                match c {
                    'p' => {
                        bt_cancel.store(false, std::sync::atomic::Ordering::Relaxed);
                        let cancel = bt_cancel.clone();
                        tasks.push(tokio::task::spawn_blocking(move || {
                            bt_pair_window(tx, cancel, busy)
                        }));
                    }
                    's' => {
                        bt_note = "Looking for speakers (12 s)... put yours in pairing mode".into();
                        tasks.push(tokio::task::spawn_blocking(move || {
                            bt_task(tx.clone(), busy, move || {
                                let found = bluetooth::scan(12);
                                let note = if found.is_empty() {
                                    "Nothing found"
                                } else {
                                    "Choose one and press Enter"
                                };
                                let _ = tx.blocking_send(Update::Bt(Bt::Found(found)));
                                Some(note.to_owned())
                            })
                        }));
                    }
                    'f' => {
                        confirm = Some(Confirm {
                            label: "Forget ALL Bluetooth pairings? Every phone, laptop and speaker must pair again.".into(),
                            body: json!({"kind":"btforget"}),
                        });
                    }
                    'r' => {
                        bt_note = "Resetting Bluetooth...".into();
                        tasks.push(tokio::task::spawn_blocking(move || {
                            bt_task(tx, busy, || {
                                Some(match bluetooth::reset_service() {
                                    Ok(()) => "Bluetooth reset".to_owned(),
                                    Err(e) => e,
                                })
                            })
                        }));
                    }
                    't' => {
                        let on = !bt_powered;
                        bt_off_by_hand = !on;
                        tasks.push(tokio::task::spawn_blocking(move || {
                            bt_task(tx, busy, move || {
                                bluetooth::set_power(on);
                                Some(format!(
                                    "Bluetooth turned {}",
                                    if on { "on" } else { "off" }
                                ))
                            })
                        }));
                    }
                    'a' => {
                        bt_note = "Sound test: listen for left, then right, then both".into();
                        tasks.push(tokio::task::spawn_blocking(move || {
                            bt_task(tx.clone(), busy, move || {
                                for (name, left, right) in endora_terminal::audio::STEPS {
                                    let _ = tx.blocking_send(Update::Bt(Bt::Note(format!(
                                        "Sound test: playing the {name}..."
                                    ))));
                                    if let Err(e) = endora_terminal::audio::play(left, right) {
                                        return Some(e);
                                    }
                                }
                                Some("Sound test done. Missing an ear here? Check the cable and the device it plays from.".to_owned())
                            })
                        }));
                    }
                    _ => {
                        if let Some(device) = bt_paired.get(selected[8]).cloned() {
                            tasks.push(tokio::task::spawn_blocking(move || {
                                bt_task(tx, busy, move || {
                                    Some(match bluetooth::forget_one(&device) {
                                        Ok(()) => format!("Forgot {}. Forget it on that device too before pairing again.", device.name),
                                        Err(e) => e,
                                    })
                                })
                            }));
                        }
                    }
                }
            }
            KeyCode::Char('m') if page == 6 && unlocked => {
                page = 8;
                next_bt = Instant::now();
            }
            KeyCode::Up | KeyCode::Down if page == 7 && unlocked => {
                // The loaded list, or the Music menu when no list is loaded.
                let rows = if music_items.is_empty() {
                    endora_terminal::presentation::MUSIC_MENU.len()
                } else {
                    music_items.len()
                };
                selected[7] = if code == KeyCode::Up {
                    selected[7].saturating_sub(1)
                } else {
                    (selected[7] + 1).min(rows.saturating_sub(1))
                };
            }
            KeyCode::Enter if page == 7 && unlocked => {
                if let Some(item) = music_items.get(selected[7]).cloned() {
                    let action = if let Some(id) = item.device_id() {
                        music_note = format!("Moving the music to {}...", item.label);
                        MusicAction::Transfer(id.to_owned())
                    } else if radio.is_some() && is_radio_title(&music_title) {
                        // Jump within the radio: play from this song on, so it keeps going.
                        music_note = "Starting...".into();
                        MusicAction::PlayTracks(
                            music_items[selected[7]..]
                                .iter()
                                .map(|t| t.uri.clone())
                                .collect(),
                        )
                    } else if item.is_artist() {
                        music_note = "Opening albums...".into();
                        music_prev = Some((music_title.clone(), music_items.clone()));
                        MusicAction::Open(item)
                    } else {
                        music_note = "Starting...".into();
                        MusicAction::Play(item)
                    };
                    let (tx, player, node) = (tx.clone(), player.clone(), client.clone());
                    tasks.push(tokio::spawn(async move {
                        let _ = tx
                            .send(Update::Music(music_work(player, node, action).await))
                            .await;
                    }));
                } else {
                    menu = true;
                }
            }
            KeyCode::Char(
                c @ (' ' | 'n' | 'b' | '+' | '=' | '-' | 'l' | 'y' | 'r' | 'a' | 't' | 'k' | 'd'
                | 'h' | 'x' | 's' | 'o'),
            ) if page == 7 && unlocked => {
                let action = match c {
                    ' ' if last_toggle.elapsed() < Duration::from_millis(700) => None,
                    ' ' => {
                        last_toggle = Instant::now();
                        Some(MusicAction::Toggle(
                            now_playing.as_ref().is_some_and(|n| n.playing),
                        ))
                    }
                    'n' => Some(MusicAction::Skip(true)),
                    'b' => Some(MusicAction::Skip(false)),
                    '+' | '=' => Some(MusicAction::Volume(
                        now_playing
                            .as_ref()
                            .and_then(|n| n.volume)
                            .unwrap_or(50)
                            .saturating_add(10)
                            .min(100),
                    )),
                    '-' => Some(MusicAction::Volume(
                        now_playing
                            .as_ref()
                            .and_then(|n| n.volume)
                            .unwrap_or(50)
                            .saturating_sub(10),
                    )),
                    'l' => Some(MusicAction::Playlists),
                    'y' => Some(MusicAction::Liked),
                    'r' => Some(MusicAction::Recent),
                    'a' => Some(MusicAction::Albums),
                    't' => Some(MusicAction::TopArtists),
                    'k' => Some(MusicAction::TopTracks),
                    'd' => {
                        music_note = "Starting the Discover radio...".into();
                        Some(MusicAction::RadioStart {
                            avoid: disliked.clone(),
                            played: radio
                                .as_ref()
                                .map(|r| r.played().to_vec())
                                .unwrap_or_default(),
                        })
                    }
                    // O lists the devices on the account to move the music to.
                    'o' => {
                        music_note = "Looking for devices...".into();
                        Some(MusicAction::Devices)
                    }
                    // S smart shuffles the highlighted playlist, or Liked songs otherwise.
                    's' => {
                        let playlist = music_items
                            .get(selected[7])
                            .filter(|i| i.uri.starts_with("spotify:playlist:"))
                            .cloned();
                        let name = playlist.as_ref().map_or_else(
                            || "Liked songs".to_owned(),
                            |p| p.label.trim_start_matches("Playlist: ").to_owned(),
                        );
                        music_note = format!("Starting a smart shuffle of {name}...");
                        Some(MusicAction::SmartStart {
                            source: playlist,
                            name,
                            avoid: disliked.clone(),
                            played: radio
                                .as_ref()
                                .map(|r| r.played().to_vec())
                                .unwrap_or_default(),
                        })
                    }
                    // H likes the song playing now (saves it to Liked songs).
                    'h' => match now_playing.as_ref().filter(|n| !n.uri.is_empty()) {
                        Some(n) => Some(MusicAction::Like(n.uri.clone())),
                        None => {
                            music_note = "Nothing is playing to like".into();
                            None
                        }
                    },
                    // X skips, and the Discover radio plays less of that artist.
                    // X dislikes: skips, and plays less of that artist from now on.
                    'x' => match now_playing.as_ref() {
                        Some(n) => {
                            if !n.artist_uri.is_empty() && !disliked.contains(&n.artist_uri) {
                                disliked.push(n.artist_uri.clone());
                            }
                            if let Some(r) = radio.as_mut() {
                                r.avoid(&n.artist_uri);
                            }
                            music_note =
                                format!("Disliked: skipped, and less of {} from now on", n.artist);
                            Some(MusicAction::Skip(true))
                        }
                        None => {
                            music_note = "Nothing is playing to dislike".into();
                            None
                        }
                    },
                    _ => None,
                };
                if matches!(c, 'l' | 'y' | 'r' | 'a' | 't' | 'k' | 'd' | 's' | 'o') {
                    music_prev = None;
                }
                if let Some(action) = action {
                    let (tx, player, node) = (tx.clone(), player.clone(), client.clone());
                    tasks.push(tokio::spawn(async move {
                        let _ = tx
                            .send(Update::Music(music_work(player, node, action).await))
                            .await;
                    }));
                }
            }
            KeyCode::Char('/') if page == 7 && unlocked => {
                wheel.close();
                drop(guard.take());
                let typed = super::prompt("Search Spotify (blank cancels): ");
                guard = Some(Screen::enter()?);
                wheel = ConsoleWheel::open();
                terminal.clear().map_err(|_| "Cannot restore screen")?;
                last_input = Instant::now();
                match typed {
                    Ok(q) if !q.trim().is_empty() => {
                        music_prev = None;
                        music_note = "Searching...".into();
                        let (tx, player, node, q) = (
                            tx.clone(),
                            player.clone(),
                            client.clone(),
                            q.trim().to_owned(),
                        );
                        tasks.push(tokio::spawn(async move {
                            let _ = tx
                                .send(Update::Music(
                                    music_work(player, node, MusicAction::Search(q)).await,
                                ))
                                .await;
                        }));
                    }
                    Err(e) => status = e,
                    _ => {}
                }
            }
            KeyCode::Up | KeyCode::Down => {
                if matches!(page, 2..=5) && !items(&data[page]).is_empty() {
                    picked = page == 3;
                    selected[page] = if code == KeyCode::Up {
                        selected[page].saturating_sub(1)
                    } else {
                        selected[page]
                            .saturating_add(1)
                            .min(items(&data[page]).len().saturating_sub(1))
                    };
                    scroll[page] = selected[page].saturating_sub(5).min(bottom as usize) as u16;
                } else {
                    scroll[page] = if code == KeyCode::Up {
                        scroll[page].saturating_sub(1)
                    } else {
                        scroll[page].saturating_add(1).min(bottom)
                    };
                }
            }
            KeyCode::PageUp | KeyCode::PageDown => {
                scroll[page] = if code == KeyCode::PageUp {
                    scroll[page].saturating_sub(10)
                } else {
                    scroll[page].saturating_add(10).min(bottom)
                };
            }
            KeyCode::Enter
                if page == 3 && unlocked && !busy && picked && !search.trim().is_empty() =>
            {
                if let Some(item) = items(&data[3]).get(selected[3]) {
                    let kind = item["media_type"].as_str().unwrap_or("");
                    if matches!(kind, "movie" | "tv")
                        && item["id"].as_str().is_some_and(|id| !id.is_empty())
                    {
                        confirm = Some(Confirm {
                            label: format!("Request {} ({kind})?", line(&item["label"])),
                            body: json!({"kind":"media","action":kind,"target":item["id"],"confirmed":true,"request_id":uuid::Uuid::new_v4().to_string()}),
                        });
                    }
                }
            }
            KeyCode::Enter if page == 3 && unlocked => {
                picked = false;
                next_read = Instant::now();
            }
            KeyCode::Char('c') if page == 2 && unlocked => {
                home_catalog = !home_catalog;
                selected[2] = 0;
                scroll[2] = 0;
                next_read = Instant::now();
            }
            KeyCode::Char(' ') if page == 2 && unlocked && !busy => {
                if let Some(item) = items(&data[2]).get(selected[2]) {
                    let action = if item["favorite"] == true || !home_catalog {
                        "remove"
                    } else {
                        "add"
                    };
                    confirm = Some(Confirm {
                        label: format!(
                            "{action} {} {}favorites?",
                            line(&item["label"]),
                            if action == "add" { "to " } else { "from " }
                        ),
                        body: json!({"request_id":uuid::Uuid::new_v4().to_string(),"kind":"favorite","action":action,"target":item["id"],"confirmed":true}),
                    });
                }
            }
            KeyCode::Char('v') if !matches!(page, 3 | 6) && unlocked && !busy => {
                let pending = endora_terminal::storage::Paths::state()?
                    .read("pending-operation")
                    .ok()
                    .and_then(|b| serde_json::from_slice::<Value>(&b).ok());
                if let Some(p) =
                    pending.filter(|p| settings.config().is_ok_and(|c| p["node"] == c.url))
                {
                    let id = p["request_id"].as_str().unwrap_or("");
                    if id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
                        let path = format!("/v1/terminal/operations/{id}");
                        let c = client.clone().unwrap();
                        let tx = tx.clone();
                        let g = epoch;
                        tasks.push(tokio::spawn(async move {
                            let _ = tx.send(Update::Action(g, c.get(&path).await)).await;
                        }));
                    }
                } else {
                    next_read = Instant::now();
                }
            }
            KeyCode::Char(c @ ('o' | 'f' | 'a' | 'r' | 's' | 't' | 'i'))
                if matches!(page, 2..=4)
                    && unlocked
                    && !busy
                    && (page != 3 || key.modifiers.contains(KeyModifiers::CONTROL)) =>
            {
                if let Some(item) = items(&data[page]).get(selected[page]) {
                    let action = match (page, c) {
                        (2, 'o') => Some(("home", "on")),
                        (2, 'f') => Some(("home", "off")),
                        (2, 'a') => Some(("home", "scene")),
                        (3, 'r') => Some(("media", item["media_type"].as_str().unwrap_or(""))),
                        (4, 's') => Some(("container", "start")),
                        (4, 't') => Some(("container", "stop")),
                        (4, 'r') => Some(("container", "restart")),
                        _ => None,
                    };
                    if page == 4 && c == 'i' {
                        let id = line(&item["id"]);
                        if id.bytes().all(|b| b.is_ascii_hexdigit()) {
                            let path = format!("/v1/terminal/logs/{id}");
                            detail = Some((4, path.clone()));
                            scroll[4] = 0;
                            let c = client.clone().unwrap();
                            let tx = tx.clone();
                            let g = epoch;
                            tasks.push(tokio::spawn(async move {
                                let _ = tx.send(Update::Read(g, 4, c.get(&path).await)).await;
                            }));
                        }
                    } else if let Some((kind, action)) = action {
                        if item["protected"] == true {
                            status = "Use SSH for control-path maintenance".into();
                        } else {
                            confirm = Some(Confirm {
                                label: format!("{action} {}?", line(&item["label"])),
                                body: json!({"kind":kind,"action":action,"target":item["id"],"confirmed":true,"request_id":uuid::Uuid::new_v4().to_string()}),
                            });
                        }
                    }
                }
            }
            KeyCode::Enter if page == 5 && unlocked && !busy => {
                if let Some(item) = items(&data[page]).get(selected[page]) {
                    confirm = Some(Confirm {
                        label: format!("Run {}?", line(&item["label"])),
                        body: json!({"kind":"job","action":"run","target":item["id"],"confirmed":true,"request_id":uuid::Uuid::new_v4().to_string()}),
                    });
                }
            }
            KeyCode::Char('z') if page == 6 => {
                detail = None;
                power.mode = if power.mode == Mode::Travel {
                    Mode::Desk
                } else {
                    Mode::Travel
                };
                settings.desk = power.mode == Mode::Desk;
                settings.save()?;
            }
            KeyCode::Char('i') if page == 5 && unlocked && !busy => {
                if let Some(id) = items(&data[5])
                    .get(selected[5])
                    .and_then(|item| item["latest_id"].as_str())
                    .filter(|id| id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'))
                {
                    let path = format!("/v1/terminal/jobs/{id}");
                    detail = Some((5, path.clone()));
                    scroll[5] = 0;
                    let c = client.clone().unwrap();
                    let tx = tx.clone();
                    let g = epoch;
                    tasks.push(tokio::spawn(async move {
                        let _ = tx.send(Update::Read(g, 5, c.get(&path).await)).await;
                    }));
                } else {
                    status = "This job has not run yet".into();
                }
            }
            KeyCode::Char('a') if page == 6 => {
                if let Some(seconds) = usage.suggestion() {
                    usage.apply();
                    usage.save()?;
                    power.pocket_seconds = seconds;
                    status = format!("Travel screen-off timeout set to {seconds}s");
                } else {
                    status = "No timing change suggested yet".into();
                }
            }
            KeyCode::Char('i') if page == 6 => {
                usage = power::Usage::default();
                usage.save()?;
                power.pocket_seconds = 180;
                status = "Private timing history reset".into();
            }
            KeyCode::Char('g') if page == 6 && unlocked => {
                confirm = Some(Confirm {
                    label: "Shut down the device? Turn power back on to restart.".into(),
                    body: json!({"kind":"shutdown"}),
                })
            }
            KeyCode::Char(
                c @ ('w' | 'n' | 's' | 'e' | 'f' | 'l' | 'x' | 'b' | 'd' | 't' | 'q' | 'v' | 'h'
                | 'o' | 'r' | 'y'),
            ) if page == 6 && !busy => {
                let initial_migration = c == 'e'
                    && !settings.away
                    && !endora_terminal::storage::Paths::state()?
                        .root
                        .join(settings.file("vault"))
                        .exists();
                if !unlocked && !matches!(c, 'w' | 'n' | 'd' | 's' | 'o') && !initial_migration {
                    status = "Unlock first with U".into();
                    continue;
                }
                wheel.close();
                drop(guard.take());
                let result: Result<()> = match c {
                    'w' => super::wifi_picker(),
                    'h' => super::desktop(),
                    'y' => match client.as_ref() {
                        Some(node) => super::music_setup(node).await,
                        None => Err("Unlock first with U".into()),
                    },
                    'o' => super::text_browser("portal", "http://neverssl.com/"),
                    'r' => {
                        let typed = super::prompt("Web address (blank returns): ")?;
                        let typed = typed.trim();
                        if typed.is_empty() {
                            Ok(())
                        } else {
                            let url = if typed.contains("://") {
                                typed.to_owned()
                            } else {
                                format!("https://{typed}")
                            };
                            endora_terminal::portable::validate_web(&url)
                                .and_then(|()| super::text_browser("web", &url))
                        }
                    }
                    'n' => Command::new("nmtui")
                        .status()
                        .map(|_| ())
                        .map_err(|_| "Wi-Fi recovery unavailable".into()),
                    's' => super::setup().await,
                    'e' => super::lock_setup().await,
                    'f' => super::remote_setup(),
                    'v' => super::bookmark_setup(),
                    't' => {
                        settings.away = !settings.away;
                        settings.config().and_then(|_| settings.save())
                    }
                    'l' => super::privileged_child(Command::new(
                        std::env::var("SHELL").unwrap_or("/bin/sh".into()),
                    )),
                    'x' => {
                        if let Some(alias) = &settings.ssh {
                            let mut command = Command::new("ssh");
                            command.arg(alias);
                            super::privileged_child(command)
                        } else {
                            Err("Configure the SSH alias through remote-setup".into())
                        }
                    }
                    'b' => {
                        for (i, b) in settings.bookmarks.iter().enumerate() {
                            println!("{} {}", i + 1, clean(&b.name));
                        }
                        let pick = super::prompt("Bookmark number (blank returns): ")?;
                        if let Some(bookmark) = pick
                            .parse::<usize>()
                            .ok()
                            .and_then(|i| i.checked_sub(1))
                            .and_then(|i| settings.bookmarks.get(i))
                        {
                            super::browser("web", &bookmark.url)
                        } else {
                            Ok(())
                        }
                    }
                    'd' => {
                        let result = super::doctor().await;
                        let _ = super::prompt("Press Enter to return: ");
                        result
                    }
                    'q' => {
                        guard = Some(Screen::enter()?);
                        break;
                    }
                    _ => Ok(()),
                };
                for task in tasks.drain(..) {
                    task.abort();
                    let _ = task.await;
                }
                epoch += 1;
                busy = false;
                reading = false;
                settings = Settings::load()?;
                // Stay unlocked after Wi-Fi, shell and browser sessions; sign-in, profile and
                // encryption changes replace the credentials, so those ask for the passcode again.
                if matches!(c, 's' | 'e' | 'f' | 't') {
                    unlocked = false;
                    client = portable::locked(&settings).ok();
                    *player.lock().await = None;
                    for d in data.iter_mut() {
                        *d = json!({});
                    }
                    page = 0;
                    prompt = Some(zeroize::Zeroizing::new(String::new()));
                }
                status = result.err().unwrap_or(if unlocked {
                    "Returned".into()
                } else {
                    "Credentials changed | enter the passcode".into()
                });
                guard = Some(Screen::enter()?);
                wheel = ConsoleWheel::open();
                terminal.clear().map_err(|_| "Cannot restore screen")?;
                last_input = Instant::now();
                next_read = Instant::now();
                if let Some(previous) = flow_return.take() {
                    page = previous;
                }
            }
            KeyCode::Char('p') if page == 6 => {}
            KeyCode::Char(c)
                if unlocked
                    && page == 3
                    && !key.modifiers.contains(KeyModifiers::CONTROL)
                    && !c.is_control() =>
            {
                let text = &mut search;
                picked = false;
                cursor = cursor.min(text.len());
                if text.len() < 4096 {
                    text.insert(cursor, c);
                    cursor += c.len_utf8();
                }
            }
            KeyCode::Backspace if unlocked && page == 3 && cursor > 0 => {
                picked = false;
                let text = &mut search;
                let at = text[..cursor].char_indices().last().map_or(0, |(i, _)| i);
                text.drain(at..cursor);
                cursor = at;
            }
            KeyCode::Left | KeyCode::Right if unlocked && page == 3 => {
                let text = &search;
                cursor = if code == KeyCode::Left {
                    text[..cursor].char_indices().last().map_or(0, |(i, _)| i)
                } else {
                    text[cursor..]
                        .chars()
                        .next()
                        .map_or(cursor, |c| cursor + c.len_utf8())
                };
            }
            KeyCode::Delete if unlocked && page == 3 => {
                let text = &mut search;
                if let Some(c) = text[cursor..].chars().next() {
                    text.drain(cursor..cursor + c.len_utf8());
                }
            }
            KeyCode::Home | KeyCode::End if unlocked && page == 3 => {
                cursor = if code == KeyCode::Home {
                    0
                } else {
                    search.len()
                };
            }
            _ => {}
        }
        // A flow started from another page always returns there, even if it could not run.
        if let Some(previous) = flow_return.take() {
            page = previous;
        }
        if page == 6 && code == KeyCode::Char('p') {
            wheel.close();
            drop(guard.take());
            let result = super::browser("portal", "http://neverssl.com/");
            guard = Some(Screen::enter()?);
            wheel = ConsoleWheel::open();
            terminal.clear().map_err(|_| "Cannot restore screen")?;
            status = result.err().unwrap_or("Returned from portal login".into());
            last_input = Instant::now();
        }
        tokio::task::yield_now().await;
    }
    bt_cancel.store(true, std::sync::atomic::Ordering::Relaxed);
    for task in tasks {
        task.abort();
        let _ = task.await;
    }
    let _ = power::backlight(Stage::Awake);
    drop(guard);
    Ok(())
}
