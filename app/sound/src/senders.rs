//! The AirPlay senders: stock OwnTone instances, one per group that has AirPlay speakers
//! (ADR 0104). Each reads its group's mix from a named pipe; writing to it starts the instance
//! playing on the speakers it has selected, and closing it stops it, freeing them. The hub
//! chooses each instance's speakers through its JSON API.
//!
//! Choosing speakers waits for the speakers to answer, so it happens on a thread of its own,
//! never on the player's. The same thread looks every couple of seconds at what each sender
//! really plays: a speaker it chose that has dropped out was taken by another device (or went
//! away), so the hub marks it borrowed and lets it go at once, before OwnTone would try to
//! take it back. And it keeps each speaker at the volume the owner set.

use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{
    Receiver, RecvTimeoutError, Sender, SyncSender, TrySendError, channel, sync_channel,
};
use std::time::Duration;

use endora_sound::application::engine::Change;
use endora_sound::application::hub::Hub;
use endora_sound::domain::{SpeakerId, SpeakerKind};
use endora_sound::infrastructure::FileStore;
use serde_json::{Value, json};

use crate::player::Senders;

/// How often the senders are looked at.
const LOOK_EVERY: Duration = Duration::from_secs(2);

/// How long an instance has to answer. Choosing speakers waits for them, so it is generous.
const ANSWERS_WITHIN: Duration = Duration::from_secs(15);

/// How many blocks may wait for a sender's pipe before new ones are dropped: about a second.
const QUEUED: usize = 50;

/// One sender: its OwnTone instance's API, and the pipe it plays.
#[derive(Debug, Clone)]
pub struct SenderPlace {
    /// The instance's JSON API, e.g. `http://host.docker.internal:3701`.
    pub api: String,
    /// The named pipe in the instance's library that it plays.
    pub pipe: PathBuf,
}

/// What an instance says about one of its outputs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    /// Its id, the same in every instance (an AirPlay speaker's is its device id).
    pub id: String,
    /// Its name, as the speaker advertises it.
    pub name: String,
    /// Whether it is selected and playing, or ready to: false once it has failed.
    pub selected: bool,
    /// Its volume, 0 to 100.
    pub volume: u8,
    /// Its kind, e.g. `AirPlay`.
    pub kind: String,
}

/// An OwnTone instance, through its JSON API.
pub trait Instance: Send {
    /// Its outputs.
    ///
    /// # Errors
    /// An instance that does not answer.
    fn outputs(&self) -> Result<Vec<Output>, String>;
    /// Selects exactly these outputs, deselecting the rest.
    ///
    /// # Errors
    /// An instance that does not answer.
    fn select(&self, ids: &[String]) -> Result<(), String>;
    /// Sets one output's volume.
    ///
    /// # Errors
    /// An instance that does not answer.
    fn volume(&self, id: &str, volume: u8) -> Result<(), String>;
}

/// A real OwnTone instance.
pub struct OwnTone {
    api: String,
    agent: ureq::Agent,
    password: Option<PathBuf>,
}

impl OwnTone {
    /// The instance at `api`, signing in with the password in `password` when there is one.
    /// The file is read each time: the senders may make it after the hub has started.
    #[must_use]
    pub fn new(api: &str, password: Option<PathBuf>) -> Self {
        Self {
            api: api.trim_end_matches('/').to_owned(),
            agent: ureq::Agent::config_builder()
                .timeout_global(Some(ANSWERS_WITHIN))
                .http_status_as_error(false)
                .build()
                .into(),
            password,
        }
    }

    fn signed(&self) -> Option<String> {
        // OwnTone's own scheme: Basic, user "admin". Only when the instance is not on a
        // trusted network for the hub.
        let password = std::fs::read_to_string(self.password.as_ref()?).ok()?;
        let password = password.trim();
        if password.is_empty() {
            return None;
        }
        Some(format!("Basic {}", base64(&format!("admin:{password}"))))
    }

    fn check(
        response: Result<ureq::http::Response<ureq::Body>, ureq::Error>,
    ) -> Result<ureq::http::Response<ureq::Body>, String> {
        let response = response.map_err(|e| e.to_string())?;
        let status = response.status().as_u16();
        if !(200..300).contains(&status) {
            return Err(format!("the sender answered {status}"));
        }
        Ok(response)
    }
}

/// Base64, for the one header that needs it.
fn base64(text: &str) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let bytes = text.as_bytes();
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(ABC[((n >> (18 - 6 * i)) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

impl Instance for OwnTone {
    fn outputs(&self) -> Result<Vec<Output>, String> {
        let mut req = self.agent.get(&format!("{}/api/outputs", self.api));
        if let Some(s) = self.signed() {
            req = req.header("authorization", &s);
        }
        let mut response = Self::check(req.call())?;
        let body: Value = response.body_mut().read_json().map_err(|e| e.to_string())?;
        Ok(parse_outputs(&body))
    }

    fn select(&self, ids: &[String]) -> Result<(), String> {
        let mut req = self.agent.put(&format!("{}/api/outputs/set", self.api));
        if let Some(s) = self.signed() {
            req = req.header("authorization", &s);
        }
        Self::check(req.send_json(json!({ "outputs": ids }))).map(|_| ())
    }

    fn volume(&self, id: &str, volume: u8) -> Result<(), String> {
        let mut req = self.agent.put(&format!("{}/api/outputs/{id}", self.api));
        if let Some(s) = self.signed() {
            req = req.header("authorization", &s);
        }
        Self::check(req.send_json(json!({ "volume": volume }))).map(|_| ())
    }
}

/// OwnTone's `{"outputs": [...]}`, read leniently: an output missing a field is skipped.
#[must_use]
pub fn parse_outputs(body: &Value) -> Vec<Output> {
    body["outputs"]
        .as_array()
        .map(|all| {
            all.iter()
                .filter_map(|o| {
                    Some(Output {
                        id: o["id"]
                            .as_str()
                            .map(str::to_owned)
                            .or_else(|| o["id"].as_u64().map(|n| n.to_string()))?,
                        name: o["name"].as_str()?.to_owned(),
                        selected: o["selected"].as_bool().unwrap_or(false),
                        volume: u8::try_from(o["volume"].as_u64().unwrap_or(0).min(100))
                            .unwrap_or(0),
                        kind: o["type"].as_str().unwrap_or("").to_owned(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// What the controller thread is told.
enum Told {
    Apply(Vec<Change>),
}

/// One sender's pipe, open while its group sounds.
struct Pipe {
    path: PathBuf,
    writer: Option<SyncSender<Vec<i16>>>,
}

impl Pipe {
    fn open(&mut self) -> &SyncSender<Vec<i16>> {
        let path = self.path.clone();
        self.writer.get_or_insert_with(|| {
            let (tx, rx) = sync_channel::<Vec<i16>>(QUEUED);
            std::thread::spawn(move || {
                // OwnTone holds the pipe open for reading while it waits for sound, so this
                // returns at once; if it is not running yet, it waits for it.
                let Ok(mut out) = std::fs::OpenOptions::new().write(true).open(&path) else {
                    return;
                };
                for block in rx {
                    let bytes: Vec<u8> = block.iter().flat_map(|s| s.to_le_bytes()).collect();
                    if out.write_all(&bytes).is_err() {
                        return;
                    }
                }
                // Every sender of blocks has gone: the group went quiet. Closing the pipe
                // stops the instance, and its speakers are free.
            });
            tx
        })
    }
}

/// The senders: the pipes the player writes, and the thread that chooses speakers.
pub struct OwnToneSenders {
    pipes: Vec<Pipe>,
    told: Sender<Told>,
}

impl OwnToneSenders {
    /// Senders at these places, choosing speakers by the names the patchbay gives them.
    #[must_use]
    pub fn start(
        hub: Arc<Hub<FileStore>>,
        places: &[SenderPlace],
        instances: Vec<Box<dyn Instance>>,
    ) -> Self {
        let (told, hears) = channel();
        let mut control = Control {
            hub,
            instances,
            wanted: BTreeMap::new(),
            chosen: BTreeMap::new(),
        };
        std::thread::spawn(move || control.run(&hears));
        Self {
            pipes: places
                .iter()
                .map(|p| Pipe {
                    path: p.pipe.clone(),
                    writer: None,
                })
                .collect(),
            told,
        }
    }
}

impl Senders for OwnToneSenders {
    fn apply(&mut self, changes: &[Change]) {
        // A sender left with nothing to play has its pipe closed, so its instance stops; the
        // next sound starts it fresh. Left open and quiet, OwnTone pauses and stays paused:
        // the second live test found a sender stuck so after its one speaker was borrowed.
        for change in changes {
            let quiet = match change {
                Change::Released { sender } => Some(*sender),
                Change::Sender { sender, speakers } if speakers.is_empty() => Some(*sender),
                Change::Sender { .. } => None,
            };
            if let Some(pipe) = quiet.and_then(|s| self.pipes.get_mut(s)) {
                pipe.writer = None;
            }
        }
        let _ = self.told.send(Told::Apply(changes.to_vec()));
    }

    fn play(&mut self, sender: usize, samples: &[i16], sounding: bool) {
        let Some(pipe) = self.pipes.get_mut(sender) else {
            return;
        };
        if !sounding {
            pipe.writer = None;
            return;
        }
        match pipe.open().try_send(samples.to_vec()) {
            Ok(()) | Err(TrySendError::Full(_)) => {}
            Err(TrySendError::Disconnected(_)) => pipe.writer = None,
        }
    }
}

/// The thread that chooses each sender's speakers and watches them.
struct Control {
    hub: Arc<Hub<FileStore>>,
    instances: Vec<Box<dyn Instance>>,
    /// The speakers each sender should play.
    wanted: BTreeMap<usize, Vec<SpeakerId>>,
    /// The output ids each sender was told to select, and found selected since.
    chosen: BTreeMap<usize, Vec<(SpeakerId, String, bool)>>,
}

impl Control {
    fn run(&mut self, hears: &Receiver<Told>) {
        loop {
            match hears.recv_timeout(LOOK_EVERY) {
                Ok(Told::Apply(changes)) => {
                    for change in changes {
                        match change {
                            Change::Sender { sender, speakers } => {
                                self.wanted.insert(sender, speakers);
                                self.choose(sender);
                            }
                            Change::Released { sender } => {
                                self.wanted.remove(&sender);
                                self.choose(sender);
                            }
                        }
                    }
                }
                Err(RecvTimeoutError::Timeout) => {
                    let senders: Vec<usize> = self.wanted.keys().copied().collect();
                    for sender in senders {
                        self.look(sender);
                    }
                }
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
    }

    /// The AirPlay speakers' names, by id, as the patchbay has them now.
    fn names(&self, speakers: &[SpeakerId]) -> Vec<(SpeakerId, String, u8)> {
        self.hub.read(|bay| {
            speakers
                .iter()
                .filter_map(|id| bay.speaker(*id))
                .filter(|s| s.kind == SpeakerKind::AirPlay)
                .map(|s| (s.id, s.name.clone(), s.volume.get()))
                .collect()
        })
    }

    /// Tells a sender's instance to play exactly the speakers it should.
    fn choose(&mut self, sender: usize) {
        let Some(instance) = self.instances.get(sender) else {
            return;
        };
        let speakers = self.wanted.get(&sender).cloned().unwrap_or_default();
        let named = self.names(&speakers);
        let outputs = match instance.outputs() {
            Ok(o) => o,
            Err(e) => {
                eprintln!("sound: sender {} did not answer: {e}", sender + 1);
                return;
            }
        };
        let mut chosen = Vec::new();
        for (id, name, _) in &named {
            match outputs.iter().find(|o| &o.name == name) {
                Some(o) => chosen.push((*id, o.id.clone(), false)),
                None => eprintln!(
                    "sound: sender {} cannot see the speaker {name} yet",
                    sender + 1
                ),
            }
        }
        let ids: Vec<String> = chosen.iter().map(|(_, o, _)| o.clone()).collect();
        if let Err(e) = instance.select(&ids) {
            eprintln!(
                "sound: sender {} would not take its speakers: {e}",
                sender + 1
            );
        }
        // Volumes after selecting: an instance gives a newly selected speaker its own idea of
        // a volume.
        for (speaker, output, _) in &chosen {
            if let Some((_, _, volume)) = named.iter().find(|(id, _, _)| id == speaker) {
                let _ = instance.volume(output, *volume);
            }
        }
        self.chosen.insert(sender, chosen);
    }

    /// Looks at what a sender plays: a speaker it chose and saw playing that has since dropped
    /// out is borrowed; a speaker's volume that differs is put back.
    fn look(&mut self, sender: usize) {
        let Some(instance) = self.instances.get(sender) else {
            return;
        };
        let Ok(outputs) = instance.outputs() else {
            return;
        };
        let speakers = self.wanted.get(&sender).cloned().unwrap_or_default();
        let named = self.names(&speakers);
        let mut borrowed = Vec::new();
        if let Some(chosen) = self.chosen.get_mut(&sender) {
            for (speaker, output, seen) in chosen.iter_mut() {
                let Some(o) = outputs.iter().find(|o| &o.id == output) else {
                    continue;
                };
                if o.selected {
                    *seen = true;
                    if let Some((_, _, volume)) = named.iter().find(|(id, _, _)| id == speaker) {
                        if o.volume != *volume {
                            let _ = instance.volume(&o.id, *volume);
                        }
                    }
                } else if *seen {
                    borrowed.push(*speaker);
                }
            }
        }
        for speaker in borrowed {
            // Let it go now, before the instance tries to take it back; marking it borrowed
            // takes it out of its group, and the plan that follows deselects it.
            if let Some(chosen) = self.chosen.get_mut(&sender) {
                chosen.retain(|(s, _, _)| *s != speaker);
                let ids: Vec<String> = chosen.iter().map(|(_, o, _)| o.clone()).collect();
                let _ = instance.select(&ids);
            }
            let _ = self.hub.change(|bay| bay.set_borrowed(speaker, true));
            println!("sound: speaker {speaker} is in use by another device; let go");
        }
    }
}

/// The file, in the pipes folder, that names every AirPlay speaker the hub knows. The senders
/// read it when they start, and tell OwnTone never to reconnect to any of them by itself:
/// a speaker another device takes is let go, and comes back only when the hub asks for it.
/// When it changes, the senders start again to read it.
pub const SPEAKER_NAMES: &str = "airplay-speakers.txt";

/// Writes the AirPlay speakers' names for the senders, one a line, when they differ from what
/// is there. Says whether it wrote.
///
/// # Errors
/// A file that cannot be written.
pub fn write_speaker_names(dir: &std::path::Path, names: &[String]) -> std::io::Result<bool> {
    let mut names: Vec<&str> = names
        .iter()
        .map(String::as_str)
        .filter(|n| !n.contains('\n'))
        .collect();
    names.sort_unstable();
    names.dedup();
    let text: String = names.iter().map(|n| format!("{n}\n")).collect();
    let path = dir.join(SPEAKER_NAMES);
    if std::fs::read_to_string(&path).is_ok_and(|now| now == text) {
        return Ok(false);
    }
    std::fs::create_dir_all(dir)?;
    let beside = dir.join(format!("{SPEAKER_NAMES}.saving"));
    std::fs::write(&beside, text)?;
    std::fs::rename(&beside, &path)?;
    Ok(true)
}

/// Makes a sender's pipe, and its folder, if they are not there.
///
/// # Errors
/// A folder or pipe that cannot be made.
pub fn make_pipe(path: &std::path::Path) -> std::io::Result<()> {
    endora_sound::infrastructure::feeds::make_pipe(path)
}

/// The AirPlay speakers an instance can see, for the owner to add: not the hub's own receivers.
///
/// # Errors
/// An instance that does not answer.
pub fn found(instance: &dyn Instance) -> Result<Vec<String>, String> {
    let mut names: Vec<String> = instance
        .outputs()?
        .into_iter()
        // OwnTone says "AirPlay 2" (or "AirPlay" for the older kind): either is a speaker.
        .filter(|o| {
            o.kind.to_ascii_lowercase().starts_with("airplay") && !o.name.starts_with("Endora")
        })
        .map(|o| o.name)
        .collect();
    names.sort();
    names.dedup();
    Ok(names)
}

#[cfg(test)]
#[path = "senders_tests.rs"]
mod tests;
