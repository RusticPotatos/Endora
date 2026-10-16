//! A small Spotify Web API client for the Music page: control playback on this device and search
//! or open playlists.
//!
//! Playback happens on the Pi itself through its Spotify Connect speaker; this module only tells
//! Spotify what to play. The sign-in is Endora's, made once on any device (plan phase 4): this
//! device borrows an access token from the node and asks again when it runs out, so the refresh
//! token never lives here. A sign-in made here before Endora held one is handed to Endora the
//! first time the node can be reached, then forgotten.
use crate::{Result, storage::Paths};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::time::Duration;

/// What to do when Spotify cannot see this device's speaker: sign it in from the handheld.
pub const SPEAKER_MISSING: &str =
    "The Speaker is not signed in yet: Settings > Accounts > Spotify sign-in, then S";

/// A sign-in screen's heading, numbered for the steps actually shown.
pub fn step_heading(step: u8, total: u8, text: &str) -> String {
    format!("Step {step} of {total}: {text}")
}

/// A successful reply's body: empty is fine (saving a song answers 200 with nothing), otherwise it
/// must be JSON.
pub fn reply_value(body: &str) -> Result<Value> {
    if body.trim().is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_str(body).map_err(|_| "Unexpected reply from Spotify".into())
}

/// A command's successful reply: JSON if it is JSON, otherwise nothing (next track answers with
/// an opaque token).
pub fn command_reply(body: &str) -> Value {
    serde_json::from_str(body).unwrap_or(Value::Null)
}

/// A device on the account that can play (phone, computer, speaker, TV, this handheld).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlayDevice {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub active: bool,
}

/// The devices from `/me/player/devices`; entries without a plain id are left out, since the id
/// goes into a request.
pub fn parse_devices(value: &Value) -> Vec<PlayDevice> {
    value["devices"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|d| {
            let id = d["id"].as_str()?;
            if id.is_empty() || id.len() > 64 || !id.bytes().all(|b| b.is_ascii_alphanumeric()) {
                return None;
            }
            Some(PlayDevice {
                id: id.to_owned(),
                name: plain(d["name"].as_str().unwrap_or("Device")),
                kind: plain(d["type"].as_str().unwrap_or("")),
                active: d["is_active"].as_bool().unwrap_or(false),
            })
        })
        .collect()
}

/// The devices as a list to pick from on the Music page.
pub fn device_items(devices: &[PlayDevice]) -> Vec<Item> {
    devices
        .iter()
        .map(|d| {
            let what = if d.name == SPEAKER {
                "this handheld".to_owned()
            } else {
                d.kind.clone()
            };
            let now = if d.active { " - playing now" } else { "" };
            Item {
                uri: format!("device:{}", d.id),
                label: format!("{} ({what}){now}", d.name),
                track: false,
            }
        })
        .collect()
}

/// Where playback goes: the chosen device if it is there, otherwise this handheld's speaker.
pub fn choose_device<'a>(
    devices: &'a [PlayDevice],
    chosen: Option<&str>,
) -> Option<&'a PlayDevice> {
    chosen
        .and_then(|name| devices.iter().find(|d| d.name == name))
        .or_else(|| devices.iter().find(|d| d.name == SPEAKER))
}

/// The name the Pi's Spotify Connect speaker shows in Spotify.
pub const SPEAKER: &str = "Speaker";

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Tokens {
    pub access: String,
    pub refresh: String,
    pub expires_at: u64,
}

impl Tokens {
    /// True a minute before expiry, so a call never starts with a token about to lapse.
    pub fn needs_refresh(&self, now: u64) -> bool {
        now + 60 >= self.expires_at
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NowPlaying {
    pub title: String,
    pub artist: String,
    pub playing: bool,
    pub device: String,
    pub volume: Option<u8>,
    pub progress_ms: u64,
    pub duration_ms: u64,
    /// The song's Spotify address, and its first artist's.
    pub uri: String,
    pub artist_uri: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub uri: String,
    pub label: String,
    /// A single track (played by uri) rather than a playlist or album (played as a context).
    pub track: bool,
}

fn encode(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

pub fn time_text(ms: u64) -> String {
    let s = ms / 1000;
    format!("{}:{:02}", s / 60, s % 60)
}

fn artists(item: &Value) -> String {
    item["artists"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|a| a["name"].as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

fn plain(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).take(80).collect()
}

fn valid_uri(uri: &str) -> bool {
    ["track", "playlist", "album", "artist"].iter().any(|kind| {
        uri.strip_prefix(&format!("spotify:{kind}:"))
            .is_some_and(|id| !id.is_empty() && id.bytes().all(|b| b.is_ascii_alphanumeric()))
    })
}

fn one_item(value: &Value) -> Option<Item> {
    let uri = value["uri"].as_str().filter(|u| valid_uri(u))?;
    let name = plain(value["name"].as_str()?);
    let (label, track) = if uri.starts_with("spotify:track:") {
        (format!("{name} - {}", plain(&artists(value))), true)
    } else if uri.starts_with("spotify:album:") {
        (format!("Album: {name} - {}", plain(&artists(value))), false)
    } else if uri.starts_with("spotify:artist:") {
        (format!("Artist: {name}"), false)
    } else {
        (format!("Playlist: {name}"), false)
    };
    Some(Item {
        uri: uri.to_owned(),
        label,
        track,
    })
}

/// Playable items from a search result, a playlist list or saved tracks.
pub fn parse_items(value: &Value) -> Vec<Item> {
    let mut out = Vec::new();
    for key in ["tracks", "playlists", "albums", "artists"] {
        for entry in value[key]["items"].as_array().into_iter().flatten() {
            out.extend(one_item(entry));
        }
    }
    for entry in value["items"].as_array().into_iter().flatten() {
        out.extend(one_item(if entry["track"].is_object() {
            &entry["track"]
        } else if entry["item"].is_object() {
            // A playlist's entries (since 2026).
            &entry["item"]
        } else if entry["album"].is_object() {
            &entry["album"]
        } else {
            entry
        }));
    }
    out
}

impl Item {
    /// The Spotify device id when this entry is a device in the "Play on" list.
    pub fn device_id(&self) -> Option<&str> {
        self.uri.strip_prefix("device:")
    }

    /// An artist opens a list of their albums instead of playing straight away.
    pub fn is_artist(&self) -> bool {
        self.uri.starts_with("spotify:artist:")
    }
}

/// Deterministic shuffle (Fisher-Yates), so a given seed always gives the same order.
pub fn shuffle<T>(items: &mut [T], seed: u64) {
    let mut state = seed | 1;
    for i in (1..items.len()).rev() {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        items.swap(i, (state % (i as u64 + 1)) as usize);
    }
}

/// How many of an artist's albums are asked for. Spotify answers "Invalid limit" above 10 for
/// development-mode (personal) apps.
pub const ARTIST_ALBUMS_LIMIT: u32 = 10;

/// The discovery list from each artist's album lookup. If every lookup failed, Spotify's error is
/// returned rather than an empty list, so the page says what went wrong.
pub fn discovery_from(
    lookups: Vec<Result<Vec<Item>>>,
    saved: &[Item],
    seed: u64,
) -> Result<Vec<Item>> {
    let mut first_error = None;
    let mut lists = Vec::new();
    for lookup in lookups {
        match lookup {
            Ok(albums) => lists.push(albums),
            Err(e) => {
                first_error.get_or_insert(e);
            }
        }
    }
    match first_error {
        Some(e) if lists.is_empty() => Err(e),
        _ => Ok(mix_discovery(&lists, saved, seed)),
    }
}

/// A discovery list: albums by the given artists that are not already saved, at most three per
/// artist, without repeats, shuffled and capped at 20. Spotify no longer offers a
/// recommendations feed, so this is built from the artists you play.
pub fn mix_discovery(by_artist: &[Vec<Item>], saved: &[Item], seed: u64) -> Vec<Item> {
    let mut seen: Vec<&str> = saved.iter().map(|i| i.uri.as_str()).collect();
    let mut mix: Vec<Item> = Vec::new();
    for albums in by_artist {
        let mut taken = 0;
        for album in albums {
            if taken == 3 {
                break;
            }
            if album.track || album.is_artist() || seen.contains(&album.uri.as_str()) {
                continue;
            }
            seen.push(&album.uri);
            mix.push(album.clone());
            taken += 1;
        }
    }
    shuffle(&mut mix, seed);
    mix.truncate(20);
    mix
}

pub fn parse_now(value: &Value) -> Option<NowPlaying> {
    let item = value.get("item").filter(|i| i.is_object())?;
    Some(NowPlaying {
        title: plain(item["name"].as_str()?),
        artist: plain(&artists(item)),
        playing: value["is_playing"].as_bool().unwrap_or(false),
        device: plain(value["device"]["name"].as_str().unwrap_or("")),
        volume: value["device"]["volume_percent"]
            .as_u64()
            .map(|v| v.min(100) as u8),
        progress_ms: value["progress_ms"].as_u64().unwrap_or(0),
        duration_ms: item["duration_ms"].as_u64().unwrap_or(0),
        uri: item["uri"]
            .as_str()
            .filter(|u| valid_uri(u))
            .unwrap_or("")
            .to_owned(),
        artist_uri: item["artists"][0]["uri"]
            .as_str()
            .filter(|u| valid_uri(u))
            .unwrap_or("")
            .to_owned(),
    })
}

/// A radio batch: a couple of songs from each album, none already played, no song twice,
/// shuffled and at most 20.
pub fn pick_tracks(
    albums: &[Vec<Item>],
    played: &[String],
    per_album: usize,
    seed: u64,
) -> Vec<Item> {
    let mut seen: Vec<String> = played.to_vec();
    let mut batch = Vec::new();
    for (i, tracks) in albums.iter().enumerate() {
        let mut choices: Vec<&Item> = tracks
            .iter()
            .filter(|t| t.track && !seen.contains(&t.uri))
            .collect();
        shuffle(&mut choices, seed.wrapping_add(i as u64 + 1));
        for track in choices.into_iter().take(per_album) {
            seen.push(track.uri.clone());
            batch.push(track.clone());
        }
    }
    shuffle(&mut batch, seed);
    batch.truncate(20);
    batch
}

/// Smart shuffle: your songs shuffled, with one new song after every `every` of them, no song
/// twice, at most 50.
pub fn smart_mix(mine: &[Item], fresh: &[Item], every: usize, seed: u64) -> Vec<Item> {
    let mut own: Vec<Item> = Vec::new();
    for t in mine.iter().filter(|t| t.track) {
        if !own.iter().any(|o| o.uri == t.uri) {
            own.push(t.clone());
        }
    }
    shuffle(&mut own, seed);
    let new: Vec<Item> = fresh
        .iter()
        .filter(|t| t.track && !own.iter().any(|o| o.uri == t.uri))
        .cloned()
        .collect();
    let mut new = new.into_iter();
    let mut mix = Vec::new();
    for (i, track) in own.into_iter().enumerate() {
        mix.push(track);
        if every > 0 && (i + 1) % every == 0 {
            if let Some(n) = new.next() {
                if !mix.iter().any(|m| m.uri == n.uri) {
                    mix.push(n);
                }
            }
        }
    }
    mix.truncate(50);
    mix
}

/// A few different pages (offsets) of a long list, chosen at random, so a big library is sampled
/// instead of downloaded whole.
pub fn sample_offsets(total: usize, page: usize, pages: usize, seed: u64) -> Vec<usize> {
    let mut offsets: Vec<usize> = (0..total).step_by(page.max(1)).collect();
    shuffle(&mut offsets, seed);
    offsets.truncate(pages);
    offsets
}

/// The Discover radio: the songs it has queued, the ones heard, and artists to play less of.
/// It fetches the next batch when the last queued song starts, and ends when something else
/// plays.
pub struct Radio {
    queue: Vec<String>,
    played: Vec<String>,
    avoided: Vec<String>,
}

impl Radio {
    pub fn new(uris: Vec<String>) -> Self {
        Self {
            queue: uris,
            played: Vec::new(),
            avoided: Vec::new(),
        }
    }

    pub fn is_ours(&self, uri: &str) -> bool {
        self.queue.iter().any(|q| q == uri)
    }

    /// True on the last queued song: time to top up.
    pub fn needs_more(&self, uri: &str) -> bool {
        self.queue.last().is_some_and(|last| last == uri)
    }

    pub fn playing(&mut self, uri: &str) {
        if !self.played.iter().any(|p| p == uri) {
            self.played.push(uri.to_owned());
            if self.played.len() > 300 {
                self.played.remove(0);
            }
        }
    }

    pub fn extend(&mut self, uris: Vec<String>) {
        self.queue.extend(uris);
    }

    pub fn avoid(&mut self, artist_uri: &str) {
        if !artist_uri.is_empty() && !self.avoided.iter().any(|a| a == artist_uri) {
            self.avoided.push(artist_uri.to_owned());
        }
    }

    pub fn played(&self) -> &[String] {
        &self.played
    }

    pub fn avoided(&self) -> &[String] {
        &self.avoided
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[derive(Serialize, Deserialize, Default)]
struct Saved {
    client_id: String,
    tokens: Option<Tokens>,
}

/// What the Music page says when there is no sign-in to use.
pub const NOT_CONNECTED: &str =
    "Spotify is not connected: press Y here, or connect it in Endora's web console";

/// Where a session's access token comes from.
enum Source {
    /// Lent by Endora, which keeps the sign-in.
    Endora(crate::client::Client),
    /// This device's own sign-in, from before Endora held one: used only while the node cannot
    /// be reached to take it over.
    Own { client_id: String, refresh: String },
}

/// A signed-in Spotify session.
pub struct Spotify {
    source: Source,
    tokens: Tokens,
    http: reqwest::Client,
    /// The device picked with "Play on" (by name); none means this handheld.
    target: Option<String>,
}

fn http() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(8))
        .timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "Cannot initialize HTTP client".into())
}

fn read_saved() -> Saved {
    Paths::state()
        .ok()
        .and_then(|p| p.read("spotify").ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn write_saved(saved: &Saved) -> Result<()> {
    let bytes = serde_json::to_vec(saved).map_err(|_| "Cannot save Spotify settings")?;
    Paths::state()?.write("spotify", &bytes)
}

/// The client id this device saved when it signed in by itself, which setup offers to Endora.
pub fn saved_client_id() -> Option<String> {
    Some(read_saved().client_id).filter(|c| !c.is_empty())
}

/// Whether Endora lent a token here last time it was asked. Kept in a file so the Music page
/// can say whether it can play without asking the node on every frame.
const LENT: &str = "spotify-lent";

fn remember_lent(lent: bool) {
    if let Ok(paths) = Paths::state() {
        let _ = if lent {
            paths.write(LENT, b"1")
        } else {
            paths.remove(LENT)
        };
    }
}

/// Whether this device still holds a sign-in of its own, from before Endora held one.
pub fn holds_own() -> bool {
    read_saved().tokens.is_some()
}

/// Whether the Music page can play: Endora lent a token lately, or this device still holds its
/// own sign-in.
pub fn is_signed_in() -> bool {
    read_saved().tokens.is_some() || Paths::state().and_then(|p| p.read(LENT)).is_ok()
}

/// What becomes of a sign-in this device holds, given where Endora's stands.
#[derive(Debug, PartialEq, Eq)]
pub enum Handover {
    /// Endora has none: it takes this one over, for every device.
    TakeOver,
    /// Endora has its own: this one is not needed, once Endora has lent a token.
    Forget,
    /// This device holds none.
    Nothing,
}

pub fn handover(endora: &str, holds_own: bool) -> Handover {
    match (holds_own, endora) {
        (false, _) => Handover::Nothing,
        (true, "connected") => Handover::Forget,
        (true, _) => Handover::TakeOver,
    }
}

/// A token Endora lent (`POST /v1/connections/spotify/token`), as this device keeps it: no
/// refresh token, which stays on Endora.
pub fn lent_tokens(value: &Value, now: u64) -> Result<Tokens> {
    let lent: endora_protocol::LentToken =
        serde_json::from_value(value.clone()).map_err(|_| "Unexpected reply from Endora")?;
    if lent.access_token.is_empty() {
        return Err("Endora lent no Spotify token".into());
    }
    Ok(Tokens {
        access: lent.access_token,
        refresh: String::new(),
        expires_at: now + u64::try_from(lent.expires_in).unwrap_or(0),
    })
}

async fn borrow(node: &crate::client::Client) -> Result<Tokens> {
    let value = node
        .post("/v1/connections/spotify/token", &json!({}))
        .await?;
    lent_tokens(&value, now_secs())
}

/// Gives this device's own sign-in to Endora when Endora has none.
async fn hand_over(node: &crate::client::Client, saved: &Saved) -> Result<()> {
    let Some(tokens) = &saved.tokens else {
        return Ok(());
    };
    let status = node.get("/v1/connections/spotify").await?;
    if handover(status["state"].as_str().unwrap_or(""), true) == Handover::TakeOver {
        node.post(
            "/v1/connections/spotify/take-over",
            &json!({"client_id": saved.client_id, "refresh_token": tokens.refresh}),
        )
        .await?;
    }
    Ok(())
}

async fn token_request(http: &reqwest::Client, form: &[(&str, &str)]) -> Result<Value> {
    let response = http
        .post("https://accounts.spotify.com/api/token")
        .form(form)
        .send()
        .await
        .map_err(|_| "Cannot reach Spotify")?;
    if !response.status().is_success() {
        return Err("Spotify refused this device's old sign-in; press Y to connect again".into());
    }
    response
        .json()
        .await
        .map_err(|_| "Unexpected reply from Spotify".into())
}

fn tokens_from(value: &Value, old_refresh: &str) -> Result<Tokens> {
    Ok(Tokens {
        access: value["access_token"]
            .as_str()
            .ok_or("No token from Spotify")?
            .to_owned(),
        refresh: value["refresh_token"]
            .as_str()
            .unwrap_or(old_refresh)
            .to_owned(),
        expires_at: now_secs() + value["expires_in"].as_u64().unwrap_or(3600),
    })
}

impl Spotify {
    /// A session with Endora's sign-in when the node lends a token. A sign-in this device still
    /// holds is handed over first, and forgotten once Endora has lent one; while the node cannot
    /// be reached, it is used as before.
    pub async fn load(node: Option<&crate::client::Client>) -> Result<Self> {
        let saved = read_saved();
        let target = Paths::state()
            .and_then(|p| p.read("spotify-device"))
            .ok()
            .map(|b| String::from_utf8_lossy(&b).trim().to_owned())
            .filter(|n| !n.is_empty());
        let mut why = NOT_CONNECTED.to_owned();
        if let Some(node) = node {
            // Its outcome shows in whether Endora then lends a token.
            let _ = hand_over(node, &saved).await;
            match borrow(node).await {
                Ok(tokens) => {
                    remember_lent(true);
                    if saved.tokens.is_some() {
                        write_saved(&Saved {
                            client_id: saved.client_id,
                            tokens: None,
                        })?;
                    }
                    return Ok(Self {
                        source: Source::Endora(node.clone()),
                        tokens,
                        http: http()?,
                        target,
                    });
                }
                Err(e) if e == "Spotify is not connected" => remember_lent(false),
                Err(e) => why = e,
            }
        }
        match saved.tokens {
            Some(tokens) if !saved.client_id.is_empty() => Ok(Self {
                source: Source::Own {
                    client_id: saved.client_id,
                    refresh: tokens.refresh.clone(),
                },
                tokens,
                http: http()?,
                target,
            }),
            _ => Err(why),
        }
    }

    /// A token with time left: borrowed again from Endora, or refreshed with this device's own
    /// sign-in. `again` replaces one Spotify stopped accepting.
    async fn fresh(&mut self, again: bool) -> Result<()> {
        if !again && !self.tokens.needs_refresh(now_secs()) {
            return Ok(());
        }
        let (client_id, refresh) = match &self.source {
            Source::Endora(node) => {
                self.tokens = borrow(node).await?;
                return Ok(());
            }
            Source::Own { client_id, refresh } => (client_id.clone(), refresh.clone()),
        };
        let value = token_request(
            &self.http,
            &[
                ("grant_type", "refresh_token"),
                ("refresh_token", &refresh),
                ("client_id", &client_id),
            ],
        )
        .await?;
        self.tokens = tokens_from(&value, &refresh)?;
        self.source = Source::Own {
            client_id: client_id.clone(),
            refresh: self.tokens.refresh.clone(),
        };
        write_saved(&Saved {
            client_id,
            tokens: Some(self.tokens.clone()),
        })
    }

    async fn send(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<&Value>,
    ) -> Result<reqwest::Response> {
        let mut request = self
            .http
            .request(method, format!("https://api.spotify.com/v1{path}"))
            .bearer_auth(&self.tokens.access);
        request = match body {
            Some(b) => request.json(b),
            None => request.header("Content-Length", "0"),
        };
        request
            .send()
            .await
            .map_err(|_| "Cannot reach Spotify".into())
    }

    async fn call(
        &mut self,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<Value> {
        self.fresh(false).await?;
        let read = method == reqwest::Method::GET;
        let mut response = self.send(method.clone(), path, body.as_ref()).await?;
        // A lent token Spotify stopped accepting (signed in again elsewhere): borrow another, once.
        if response.status().as_u16() == 401 && matches!(self.source, Source::Endora(_)) {
            self.fresh(true).await?;
            response = self.send(method, path, body.as_ref()).await?;
        }
        match response.status().as_u16() {
            // A read must answer JSON; a command (play, next, like...) only has to succeed.
            200 if read => reply_value(&response.text().await.unwrap_or_default()),
            200 => Ok(command_reply(&response.text().await.unwrap_or_default())),
            202 | 204 => Ok(Value::Null),
            401 => Err("Spotify sign-in expired; press Y to connect it again".into()),
            403 => Err("Spotify needs Premium for this".into()),
            404 => Err(SPEAKER_MISSING.to_owned()),
            429 => Err("Spotify is busy; try again in a moment".into()),
            _ => Err("Spotify could not do that".into()),
        }
    }

    pub async fn now(&mut self) -> Result<Option<NowPlaying>> {
        Ok(parse_now(
            &self.call(reqwest::Method::GET, "/me/player", None).await?,
        ))
    }

    /// The id of this device's Spotify Connect speaker, if Spotify can see it.
    /// The device playback goes to: the one picked with "Play on", or this handheld's speaker.
    async fn speaker_id(&mut self) -> Result<String> {
        let devices = self.devices().await?;
        choose_device(&devices, self.target.as_deref())
            .map(|d| d.id.clone())
            .ok_or_else(|| SPEAKER_MISSING.to_owned())
    }

    pub async fn devices(&mut self) -> Result<Vec<PlayDevice>> {
        let value = self
            .call(reqwest::Method::GET, "/me/player/devices", None)
            .await?;
        Ok(parse_devices(&value))
    }

    /// Moves playback to a device and keeps playing there; later play, next and search go there
    /// too (remembered between runs). Picking this handheld's speaker clears the choice.
    pub async fn transfer(&mut self, id: &str) -> Result<String> {
        let devices = self.devices().await?;
        let device = devices
            .iter()
            .find(|d| d.id == id)
            .ok_or("That device is not available any more")?
            .clone();
        self.call(
            reqwest::Method::PUT,
            "/me/player",
            Some(json!({"device_ids": [device.id], "play": true})),
        )
        .await?;
        self.target = (device.name != SPEAKER).then(|| device.name.clone());
        if let Ok(paths) = crate::storage::Paths::state() {
            let _ = paths.write(
                "spotify-device",
                self.target.as_deref().unwrap_or("").as_bytes(),
            );
        }
        Ok(device.name)
    }

    pub async fn pause(&mut self) -> Result<()> {
        self.call(reqwest::Method::PUT, "/me/player/pause", None)
            .await
            .map(|_| ())
    }

    pub async fn resume(&mut self) -> Result<()> {
        let id = self.speaker_id().await?;
        self.call(
            reqwest::Method::PUT,
            &format!("/me/player/play?device_id={id}"),
            Some(json!({})),
        )
        .await
        .map(|_| ())
    }

    pub async fn skip(&mut self, forward: bool) -> Result<()> {
        let path = if forward {
            "/me/player/next"
        } else {
            "/me/player/previous"
        };
        let method = reqwest::Method::POST;
        self.call(method, path, None).await.map(|_| ())
    }

    pub async fn volume(&mut self, percent: u8) -> Result<()> {
        self.call(
            reqwest::Method::PUT,
            &format!("/me/player/volume?volume_percent={}", percent.min(100)),
            None,
        )
        .await
        .map(|_| ())
    }

    pub async fn play(&mut self, item: &Item) -> Result<()> {
        let id = self.speaker_id().await?;
        let body = if item.track {
            json!({"uris": [item.uri]})
        } else {
            json!({"context_uri": item.uri})
        };
        self.call(
            reqwest::Method::PUT,
            &format!("/me/player/play?device_id={id}"),
            Some(body),
        )
        .await
        .map(|_| ())
    }

    pub async fn search(&mut self, query: &str) -> Result<Vec<Item>> {
        let path = format!(
            "/search?q={}&type=track,playlist,album,artist&limit=5",
            encode(query)
        );
        Ok(parse_items(
            &self.call(reqwest::Method::GET, &path, None).await?,
        ))
    }

    pub async fn playlists(&mut self) -> Result<Vec<Item>> {
        Ok(parse_items(
            &self
                .call(reqwest::Method::GET, "/me/playlists?limit=30", None)
                .await?,
        ))
    }

    pub async fn liked(&mut self) -> Result<Vec<Item>> {
        Ok(parse_items(
            &self
                .call(reqwest::Method::GET, "/me/tracks?limit=30", None)
                .await?,
        ))
    }

    pub async fn saved_albums(&mut self) -> Result<Vec<Item>> {
        self.browse("/me/albums?limit=30").await
    }

    pub async fn top_artists(&mut self) -> Result<Vec<Item>> {
        self.browse("/me/top/artists?limit=20&time_range=medium_term")
            .await
    }

    pub async fn top_tracks(&mut self) -> Result<Vec<Item>> {
        self.browse("/me/top/tracks?limit=30&time_range=medium_term")
            .await
    }

    pub async fn artist_albums(&mut self, artist: &Item) -> Result<Vec<Item>> {
        let id = artist
            .uri
            .strip_prefix("spotify:artist:")
            .filter(|_| valid_uri(&artist.uri))
            .ok_or("Not an artist")?;
        self.browse(&format!(
            "/artists/{id}/albums?include_groups=album,single&limit={ARTIST_ALBUMS_LIMIT}"
        ))
        .await
    }

    /// Albums from artists you play that are not saved yet, a few per artist, shuffled.
    pub async fn discover(&mut self) -> Result<Vec<Item>> {
        let mut top = self.top_artists().await?;
        if top.is_empty() {
            return Err("Play some music first; discovery starts from the artists you play".into());
        }
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(1, |d| d.as_nanos() as u64);
        shuffle(&mut top, seed);
        let mut lookups = Vec::new();
        for artist in top.iter().take(6) {
            lookups.push(self.artist_albums(artist).await);
        }
        let saved = self.saved_albums().await.unwrap_or_default();
        discovery_from(lookups, &saved, seed)
    }

    pub async fn album_tracks(&mut self, album: &Item) -> Result<Vec<Item>> {
        let id = album
            .uri
            .strip_prefix("spotify:album:")
            .filter(|_| valid_uri(&album.uri))
            .ok_or("Not an album")?;
        self.browse(&format!("/albums/{id}/tracks?limit=10")).await
    }

    /// A batch of songs for the Discover radio: from albums by artists you play (not those you
    /// skipped), not saved yet, two songs each, none already heard.
    pub async fn discover_tracks(
        &mut self,
        avoid: &[String],
        played: &[String],
    ) -> Result<Vec<Item>> {
        let mut top: Vec<Item> = self
            .top_artists()
            .await?
            .into_iter()
            .filter(|a| !avoid.contains(&a.uri))
            .collect();
        if top.is_empty() {
            return Err("Play some music first; discovery starts from the artists you play".into());
        }
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(1, |d| d.as_nanos() as u64);
        shuffle(&mut top, seed);
        let mut lookups = Vec::new();
        for artist in top.iter().take(6) {
            lookups.push(self.artist_albums(artist).await);
        }
        let saved = self.saved_albums().await.unwrap_or_default();
        let albums = discovery_from(lookups, &saved, seed)?;
        let mut tracks = Vec::new();
        for album in albums.iter().take(8) {
            if let Ok(list) = self.album_tracks(album).await {
                tracks.push(list);
            }
        }
        let batch = pick_tracks(&tracks, played, 2, seed);
        if batch.is_empty() {
            return Err("No new songs found; play some more music first".into());
        }
        Ok(batch)
    }

    /// A sample of a playlist's songs (a few random pages of 100).
    pub async fn playlist_tracks(&mut self, playlist: &Item) -> Result<Vec<Item>> {
        let id = playlist
            .uri
            .strip_prefix("spotify:playlist:")
            .filter(|_| valid_uri(&playlist.uri))
            .ok_or("Not a playlist")?;
        self.sampled(&format!("/playlists/{id}/items"), 100, 3)
            .await
    }

    /// A sample of Liked songs (a few random pages of 50).
    pub async fn liked_sample(&mut self) -> Result<Vec<Item>> {
        self.sampled("/me/tracks", 50, 4).await
    }

    /// The first page tells the size; then a few random pages are read.
    async fn sampled(&mut self, path: &str, page: usize, pages: usize) -> Result<Vec<Item>> {
        let first = self
            .call(reqwest::Method::GET, &format!("{path}?limit={page}"), None)
            .await?;
        let total = first["total"].as_u64().unwrap_or(0) as usize;
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(1, |d| d.as_nanos() as u64);
        let mut songs = parse_items(&first);
        for offset in sample_offsets(total, page, pages, seed) {
            if offset == 0 {
                continue;
            }
            if let Ok(more) = self
                .call(
                    reqwest::Method::GET,
                    &format!("{path}?limit={page}&offset={offset}"),
                    None,
                )
                .await
            {
                songs.extend(parse_items(&more));
            }
        }
        Ok(songs)
    }

    /// Adds songs to the end of the play queue, finding the speaker once.
    pub async fn queue_all(&mut self, uris: &[String]) -> Result<usize> {
        let id = self.speaker_id().await?;
        let mut queued = 0;
        for uri in uris {
            if self
                .call(
                    reqwest::Method::POST,
                    &format!("/me/player/queue?uri={}&device_id={id}", encode(uri)),
                    None,
                )
                .await
                .is_ok()
            {
                queued += 1;
            }
        }
        Ok(queued)
    }

    /// Plays these songs in order on the device's speaker.
    pub async fn play_tracks(&mut self, uris: &[String]) -> Result<()> {
        let id = self.speaker_id().await?;
        self.call(
            reqwest::Method::PUT,
            &format!("/me/player/play?device_id={id}"),
            Some(json!({ "uris": uris })),
        )
        .await
        .map(|_| ())
    }

    /// Adds a song to the end of the play queue.
    pub async fn queue(&mut self, uri: &str) -> Result<()> {
        let id = self.speaker_id().await?;
        self.call(
            reqwest::Method::POST,
            &format!("/me/player/queue?uri={}&device_id={id}", encode(uri)),
            None,
        )
        .await
        .map(|_| ())
    }

    /// Saves a song to Liked songs.
    pub async fn like(&mut self, uri: &str) -> Result<()> {
        if !valid_uri(uri) {
            return Err("Nothing to like yet".into());
        }
        self.call(
            reqwest::Method::PUT,
            "/me/library",
            Some(json!({ "uris": [uri] })),
        )
        .await
        .map(|_| ())
        .map_err(|e| {
            if e.contains("Premium") {
                "Allow saving songs: sign in to Spotify again (Settings > Accounts > Spotify)"
                    .to_owned()
            } else {
                e
            }
        })
    }

    /// A browsing call; a refusal usually means the sign-in predates the browse permission.
    async fn browse(&mut self, path: &str) -> Result<Vec<Item>> {
        let value = self
            .call(reqwest::Method::GET, path, None)
            .await
            .map_err(|e| {
                if e.contains("Premium") {
                    "Allow browsing: sign in to Spotify again (Settings > Accounts > Spotify)"
                        .to_owned()
                } else {
                    e
                }
            })?;
        Ok(parse_items(&value))
    }

    pub async fn recent(&mut self) -> Result<Vec<Item>> {
        Ok(parse_items(
            &self
                .call(
                    reqwest::Method::GET,
                    "/me/player/recently-played?limit=30",
                    None,
                )
                .await?,
        ))
    }
}

/// The sign-in link the Spotify player (librespot) prints for its own sign-in, from a line of its
/// output. Only a Spotify sign-in page is accepted, since it is shown as a QR code to scan.
pub fn speaker_link(line: &str) -> Option<String> {
    let url = line.trim().strip_prefix("Browse to:")?.trim();
    (url.starts_with("https://accounts.spotify.com/authorize?")
        && !url.chars().any(|c| c.is_whitespace() || c.is_control()))
    .then(|| url.to_owned())
}

/// The request path to hand to the player's local sign-in listener, from the address the browser
/// ended on (pasted by hand). Only `127.0.0.1:<port>/login` with a code is accepted, so the paste
/// can never send anything elsewhere or smuggle in another request.
pub fn speaker_login_path(pasted: &str, port: u16) -> Result<String> {
    let text = pasted.trim().trim_matches(['\'', '"', '<', '>']).trim();
    let text = text.strip_prefix("http://").unwrap_or(text);
    let path = text
        .strip_prefix(&format!("127.0.0.1:{port}"))
        .filter(|p| p.starts_with("/login?"))
        .ok_or("That is not the address the speaker sign-in ended on")?;
    if path.contains("error=") {
        return Err("Spotify sign-in was cancelled".into());
    }
    if !path.contains("code=")
        || !path
            .chars()
            .all(|c| c.is_ascii_graphic() && !matches!(c, '<' | '>' | '"' | '\\'))
    {
        return Err("That address is not a complete sign-in".into());
    }
    Ok(path.to_owned())
}

/// One full screen for a QR code: the screen is cleared first so earlier output cannot scroll the
/// code out of view (the console is only 45 rows tall), with a heading above and a note below.
pub fn qr_page(heading: &str, qr: &[String], footer: &str) -> String {
    let mut page = format!("\x1b[2J\x1b[H{heading}\n");
    for line in qr {
        page.push_str(line);
        page.push('\n');
    }
    page.push_str(footer);
    page.push('\n');
    page
}

/// The link as a QR code made of block characters. Light modules are drawn as filled blocks, so
/// it reads correctly as light-on-dark terminal text.
pub fn qr_lines(text: &str) -> Result<Vec<String>> {
    use qrcode::{EcLevel, QrCode, render::unicode::Dense1x2};
    let code = QrCode::with_error_correction_level(text.as_bytes(), EcLevel::L)
        .map_err(|_| "That link is too long for a QR code")?;
    let image = code
        .render::<Dense1x2>()
        .dark_color(Dense1x2::Light)
        .light_color(Dense1x2::Dark)
        .quiet_zone(true)
        .build();
    Ok(image.lines().map(str::to_owned).collect())
}

/// `%XX` and `+` escapes of a form body; anything malformed is kept as typed.
pub fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len()
                && bytes[i + 1].is_ascii_hexdigit()
                && bytes[i + 2].is_ascii_hexdigit() =>
            {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("00");
                out.push(u8::from_str_radix(hex, 16).unwrap_or(b'%'));
                i += 2;
            }
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn form_value(body: &str, key: &str) -> Option<String> {
    body.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        (percent_decode(k) == key).then(|| percent_decode(v))
    })
}

/// The address of this device on the local network, if it has one.
pub fn local_ip() -> Option<std::net::IpAddr> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    // A documentation-only address: connecting a UDP socket sends nothing, it only picks a route.
    socket
        .connect((std::net::Ipv4Addr::new(192, 0, 2, 1), 9))
        .ok()?;
    let ip = socket.local_addr().ok()?.ip();
    (!ip.is_loopback() && !ip.is_unspecified()).then_some(ip)
}

fn respond(stream: &mut std::net::TcpStream, status: &str, body: &str) {
    use std::io::Write;
    let _ = write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
}

/// The page the phone opens from the QR code: optionally a button for the Spotify sign-in (a link
/// too long for a QR code on this screen), then the box to paste the address of the page that
/// would not load. Only a Spotify sign-in link is ever offered, escaped for HTML.
pub fn paste_page(sign_in: Option<&str>) -> String {
    let link = sign_in
        .filter(|l| l.starts_with("https://accounts.spotify.com/authorize?"))
        .filter(|l| {
            !l.chars()
                .any(|c| c.is_whitespace() || matches!(c, '"' | '<' | '>'))
        })
        .map(|l| l.replace('&', "&amp;"));
    let mut page = String::from(
        "<!doctype html><meta name=viewport content=\"width=device-width,initial-scale=1\"><title>Player</title><body style=\"font-family:sans-serif;margin:1em\">",
    );
    match link {
        Some(link) => page.push_str(&format!(
            "<h3>Sign in to Spotify</h3><p>1. <a href=\"{link}\" target=_blank style=\"font-size:1.2em\">Open the Spotify sign-in</a>, sign in and press Agree.</p><p>2. The page after Agree will not load. That is expected: copy its full address, come back to this tab and paste it here.</p>"
        )),
        None => page.push_str(
            "<h3>Paste the address</h3><p>Paste the full address of the page that could not be reached.</p>",
        ),
    }
    page.push_str("<form method=post><textarea name=url rows=6 style=\"width:100%\"></textarea><p><button style=\"font-size:1.2em\">Send</button></form>");
    page
}

/// Serves a one-question page at `/token` and returns what was pasted into it. Only that exact
/// path answers; a wrong path gets 404, bodies are capped, and it gives up after `timeout`.
pub fn wait_for_paste(
    listener: &std::net::TcpListener,
    token: &str,
    timeout: Duration,
) -> Result<String> {
    wait_for_paste_with(listener, token, timeout, || false)
}

/// As [`wait_for_paste`], and `cancelled` is asked while nothing is arriving; true gives up.
pub fn wait_for_paste_with(
    listener: &std::net::TcpListener,
    token: &str,
    timeout: Duration,
    cancelled: impl FnMut() -> bool,
) -> Result<String> {
    serve_paste(listener, token, &paste_page(None), timeout, cancelled)
}

/// As [`wait_for_paste_with`], with a button for the Spotify sign-in link above the paste box, so
/// one QR code does the whole sign-in.
pub fn wait_for_sign_in(
    listener: &std::net::TcpListener,
    token: &str,
    sign_in: &str,
    timeout: Duration,
    cancelled: impl FnMut() -> bool,
) -> Result<String> {
    serve_paste(
        listener,
        token,
        &paste_page(Some(sign_in)),
        timeout,
        cancelled,
    )
}

fn serve_paste(
    listener: &std::net::TcpListener,
    token: &str,
    page: &str,
    timeout: Duration,
    mut cancelled: impl FnMut() -> bool,
) -> Result<String> {
    use std::io::Read;
    listener
        .set_nonblocking(true)
        .map_err(|_| "Cannot listen for the paste")?;
    let end = std::time::Instant::now() + timeout;
    let want = format!("/{token}");
    while std::time::Instant::now() < end {
        let Ok((mut stream, _)) = listener.accept() else {
            if cancelled() {
                return Err("Cancelled".into());
            }
            std::thread::sleep(Duration::from_millis(50));
            continue;
        };
        let _ = stream.set_nonblocking(false);
        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
        let mut data = Vec::new();
        let mut chunk = [0u8; 2048];
        let (head_end, length) = loop {
            let Ok(n) = stream.read(&mut chunk) else {
                break (None, 0);
            };
            if n == 0 {
                break (None, 0);
            }
            data.extend_from_slice(&chunk[..n]);
            if data.len() > 16 * 1024 {
                break (None, 0);
            }
            if let Some(pos) = data.windows(4).position(|w| w == b"\r\n\r\n") {
                let head = String::from_utf8_lossy(&data[..pos]).to_lowercase();
                let length = head
                    .lines()
                    .find_map(|l| l.strip_prefix("content-length:"))
                    .and_then(|v| v.trim().parse::<usize>().ok())
                    .unwrap_or(0);
                break (Some(pos + 4), length);
            }
        };
        let Some(body_at) = head_end else {
            respond(&mut stream, "400 Bad Request", "");
            continue;
        };
        while data.len() < body_at + length && data.len() <= 16 * 1024 {
            match stream.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(n) => data.extend_from_slice(&chunk[..n]),
            }
        }
        let text = String::from_utf8_lossy(&data).into_owned();
        let mut first = text.lines().next().unwrap_or("").split_whitespace();
        let (method, path) = (first.next().unwrap_or(""), first.next().unwrap_or(""));
        if path != want {
            respond(&mut stream, "404 Not Found", "");
        } else if method == "GET" {
            respond(&mut stream, "200 OK", page);
        } else if method == "POST" {
            let body = text.get(body_at..).unwrap_or("");
            match form_value(body, "url").filter(|v| !v.trim().is_empty()) {
                Some(value) => {
                    respond(
                        &mut stream,
                        "200 OK",
                        "<!doctype html><meta name=viewport content=\"width=device-width,initial-scale=1\"><body style=\"font-family:sans-serif;margin:1em\"><h3>Done</h3><p>You can close this page and look at the handheld.</p>",
                    );
                    return Ok(value);
                }
                None => respond(&mut stream, "400 Bad Request", "Nothing was pasted"),
            }
        } else {
            respond(&mut stream, "404 Not Found", "");
        }
    }
    Err("Timed out waiting for the pasted address".into())
}
