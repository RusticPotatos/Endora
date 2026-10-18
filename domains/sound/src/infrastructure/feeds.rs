//! The sources' sound, as it arrives: a buffer per source, filled by a thread of its own.
//!
//! - A radio station is decoded by `ffmpeg`, restarted with a growing wait when it drops,
//!   which is the station's watchdog.
//! - An AirPlay receiver or Spotify writes into a named pipe, read as it comes.
//!
//! Everything arrives as PCM: 44.1 kHz, 16-bit little-endian, stereo, interleaved.

use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use crate::application::engine::Feeds;
use crate::domain::mix::{CHANNELS, RATE};
use crate::domain::{Patchbay, Source, SourceId, SourceKind};

/// The most a buffer holds, in samples: two seconds. A source that runs ahead (a station
/// bursting at the start) loses its oldest sound rather than falling ever further behind.
const MOST: usize = 2 * RATE as usize * CHANNELS;

/// How much a buffer gathers after running dry before it plays again: a fifth of a second,
/// so a source that trickles does not crackle.
const PRIME: usize = RATE as usize / 5 * CHANNELS;

/// The protocols a station may use. Not `file`, not `concat`: a playlist must not be able
/// to point `ffmpeg` at something on the hub's own disk.
const STATION_PROTOCOLS: &str = "http,https,tls,tcp,crypto";

/// The most Endora's speech may queue, in samples: two minutes.
const MOST_SPOKEN: usize = 120 * RATE as usize * CHANNELS;

/// One source's sound, between the thread that fills it and the engine that takes it.
#[derive(Debug)]
pub struct Buffer {
    samples: VecDeque<i16>,
    primed: bool,
    most: usize,
    prime: usize,
}

impl Default for Buffer {
    fn default() -> Self {
        Self::live()
    }
}

impl Buffer {
    /// A buffer for live sound: at most [`MOST`], gathering [`PRIME`] after running dry.
    #[must_use]
    pub const fn live() -> Self {
        Self {
            samples: VecDeque::new(),
            primed: false,
            most: MOST,
            prime: PRIME,
        }
    }

    /// A buffer for Endora's speech, which arrives whole and faster than it plays: it keeps
    /// all of it (up to two minutes) and plays it at once.
    #[must_use]
    pub const fn spoken() -> Self {
        Self {
            samples: VecDeque::new(),
            primed: false,
            most: MOST_SPOKEN,
            prime: 0,
        }
    }

    /// Adds what arrived; past its most, the oldest goes.
    pub fn push(&mut self, samples: &[i16]) {
        self.samples.extend(samples);
        let over = self.samples.len().saturating_sub(self.most);
        self.samples.drain(..over);
        if !self.samples.is_empty() && self.samples.len() >= self.prime {
            self.primed = true;
        }
    }

    /// Takes up to `n` samples. Nothing until it has gathered enough since it last ran dry,
    /// and always whole frames.
    pub fn take(&mut self, n: usize) -> Vec<i16> {
        if !self.primed {
            return Vec::new();
        }
        let n = n.min(self.samples.len()) / CHANNELS * CHANNELS;
        let out: Vec<i16> = self.samples.drain(..n).collect();
        if self.samples.len() < CHANNELS {
            self.primed = false;
        }
        out
    }

    /// Whether it holds as much as it keeps: its reader should wait.
    #[must_use]
    pub fn full(&self) -> bool {
        self.samples.len() >= self.most
    }

    /// How many more samples it keeps before it is full.
    #[must_use]
    pub fn room(&self) -> usize {
        self.most.saturating_sub(self.samples.len())
    }

    /// How many samples it holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    /// Whether it holds nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }
}

type Shared = Arc<Mutex<Buffer>>;

fn lock(b: &Shared) -> std::sync::MutexGuard<'_, Buffer> {
    b.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Reads PCM bytes from `from` into `into` until it ends or `stop` is set. An odd byte at the
/// end of one read is kept for the next.
///
/// **It does not read while the buffer is full.** What writes the pipe is paced by that: a
/// writer that decodes faster than it plays (librespot does, and relies on its output to
/// hold it back) waits, and so plays in real time. Reading as fast as it could and dropping
/// the oldest let librespot race through whole songs in moments: the owner heard a second of
/// each, one after another.
fn pump(mut from: impl std::io::Read, into: &Shared, stop: &AtomicBool) {
    let mut bytes = [0u8; 8192];
    let mut carry: Option<u8> = None;
    while !stop.load(Ordering::Relaxed) {
        // Only as much as fits: reading a whole chunk into a nearly full buffer would push its
        // oldest out, and the writer would run ahead by what was lost (twice real time, the
        // first try showed).
        let room = lock(into).room() * 2;
        if room < 2 {
            std::thread::sleep(Duration::from_millis(5));
            continue;
        }
        let most = room.min(bytes.len());
        let n = match from.read(&mut bytes[..most]) {
            Ok(0) | Err(_) => return,
            Ok(n) => n,
        };
        let mut chunk: Vec<u8> = Vec::with_capacity(n + 1);
        chunk.extend(carry.take());
        chunk.extend_from_slice(&bytes[..n]);
        if chunk.len() % 2 == 1 {
            carry = chunk.pop();
        }
        let samples: Vec<i16> = chunk
            .chunks_exact(2)
            .map(|b| i16::from_le_bytes([b[0], b[1]]))
            .collect();
        lock(into).push(&samples);
    }
}

/// The arguments `ffmpeg` gets to play a station: no shell, network protocols only, and
/// PCM out on its standard output.
#[must_use]
pub fn station_arguments(address: &str) -> Vec<String> {
    [
        "-nostdin",
        "-hide_banner",
        "-loglevel",
        "error",
        "-protocol_whitelist",
        STATION_PROTOCOLS,
        "-reconnect",
        "1",
        "-reconnect_streamed",
        "1",
        "-reconnect_delay_max",
        "30",
        "-i",
        address,
        "-vn",
        "-f",
        "s16le",
        "-acodec",
        "pcm_s16le",
        "-ar",
        "44100",
        "-ac",
        "2",
        "pipe:1",
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect()
}

/// How long to wait before starting a station again after its `n`th drop in a row: one
/// second, doubling, never more than a minute.
#[must_use]
pub fn backoff(n: u32) -> Duration {
    Duration::from_secs((1u64 << n.min(6)).min(60))
}

/// A station played by `ffmpeg`, started again whenever it stops.
struct Station {
    stop: Arc<AtomicBool>,
    child: Arc<Mutex<Option<Child>>>,
}

impl Station {
    fn start(ffmpeg: &Path, address: &str, into: Shared) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let child: Arc<Mutex<Option<Child>>> = Arc::new(Mutex::new(None));
        let (ffmpeg, address) = (ffmpeg.to_owned(), address.to_owned());
        let (stopping, running) = (stop.clone(), child.clone());
        std::thread::spawn(move || {
            let mut drops = 0;
            while !stopping.load(Ordering::Relaxed) {
                let started = std::time::Instant::now();
                let spawned = Command::new(&ffmpeg)
                    .args(station_arguments(&address))
                    .stdin(Stdio::null())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::null())
                    .spawn();
                if let Ok(mut c) = spawned {
                    let out = c.stdout.take();
                    *running.lock().unwrap_or_else(PoisonError::into_inner) = Some(c);
                    if let Some(out) = out {
                        pump(out, &into, &stopping);
                    }
                    if let Some(mut c) = running
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .take()
                    {
                        let _ = c.kill();
                        let _ = c.wait();
                    }
                }
                // A station that played a good while before it dropped starts again quickly.
                drops = if started.elapsed() > Duration::from_secs(120) {
                    0
                } else {
                    drops + 1
                };
                let wait = backoff(drops);
                let until = std::time::Instant::now() + wait;
                while std::time::Instant::now() < until && !stopping.load(Ordering::Relaxed) {
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
        });
        Self { stop, child }
    }
}

impl Drop for Station {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(mut c) = self
            .child
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
        {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

/// A named pipe something else writes PCM into: an AirPlay receiver, or Spotify.
struct Pipe {
    stop: Arc<AtomicBool>,
    path: PathBuf,
}

/// The file name of a source's pipe, in the pipes folder: `spotify.pcm`, `airplay-<device>.pcm`
/// (or `airplay-<id>.pcm` for a receiver of no device of the owner's, a guest's), `<id>.pcm`.
#[must_use]
pub fn pipe_name(source: &Source) -> String {
    match source.kind {
        SourceKind::Spotify => "spotify.pcm".to_owned(),
        SourceKind::AirPlay => format!(
            "airplay-{}.pcm",
            source
                .device
                .as_deref()
                .map_or_else(|| source.id.to_string(), safe)
        ),
        _ => format!("{}.pcm", source.id),
    }
}

/// A name a device gives, as a file name: letters, digits and dashes, the rest as `_`.
fn safe(device: &str) -> String {
    device
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Makes sure `path` is a named pipe: makes it if it is missing, and replaces an ordinary file
/// a writer made there by starting first (librespot does, given a path with nothing at it).
///
/// # Errors
/// A pipe that cannot be made.
pub fn make_pipe(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::FileTypeExt as _;
    if let Ok(meta) = std::fs::symlink_metadata(path) {
        if meta.file_type().is_fifo() {
            return Ok(());
        }
        std::fs::remove_file(path)?;
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let made = Command::new("mkfifo")
        .arg("-m")
        .arg("0666")
        .arg(path)
        .status()?;
    if !made.success() {
        return Err(std::io::Error::other("mkfifo failed"));
    }
    Ok(())
}

impl Pipe {
    fn start(path: &Path, into: Shared) -> Self {
        if let Err(e) = make_pipe(path) {
            eprintln!("sound: cannot make the pipe {}: {e}", path.display());
        }
        let stop = Arc::new(AtomicBool::new(false));
        let (stopping, at) = (stop.clone(), path.to_owned());
        std::thread::spawn(move || {
            while !stopping.load(Ordering::Relaxed) {
                // Opening blocks until a writer opens the other end; a missing pipe is waited for.
                match std::fs::File::open(&at) {
                    Ok(f) => pump(f, &into, &stopping),
                    Err(_) => std::thread::sleep(Duration::from_secs(1)),
                }
            }
        });
        Self {
            stop,
            path: path.to_owned(),
        }
    }
}

impl Drop for Pipe {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        // The reader may be waiting in `open` for a writer: be one for a moment, so it wakes,
        // sees it is stopped, and ends.
        let _ = std::fs::OpenOptions::new().write(true).open(&self.path);
    }
}

enum Running {
    Station(#[expect(dead_code, reason = "held for its Drop, which stops the station")] Station),
    Pipe(#[expect(dead_code, reason = "held for its Drop, which stops the reader")] Pipe),
    /// Endora's voice is handed in as it is spoken (see [`FeedSet::say`]).
    Voice,
}

/// Where the hub finds what it needs to read the sources.
#[derive(Debug, Clone)]
pub struct Places {
    /// The `ffmpeg` to run.
    pub ffmpeg: PathBuf,
    /// The directory of named pipes, one per AirPlay receiver or Spotify source, named
    /// `<source id>.pcm`.
    pub pipes: PathBuf,
}

/// Every source the running groups play, each with its buffer and its reader.
pub struct FeedSet {
    places: Places,
    open: BTreeMap<SourceId, (Shared, Running)>,
}

impl FeedSet {
    /// No sources open yet.
    #[must_use]
    pub fn new(places: Places) -> Self {
        Self {
            places,
            open: BTreeMap::new(),
        }
    }

    /// The named pipe a source is read from.
    ///
    /// Named for what writes it, so its writer knows where to without asking the hub:
    /// `spotify.pcm` for Spotify, `airplay-<device>.pcm` for a device's AirPlay receiver.
    #[must_use]
    pub fn pipe_of(&self, source: &Source) -> PathBuf {
        self.places.pipes.join(pipe_name(source))
    }

    /// Opens the sources in `wanted` that are not open, and closes those open that are not
    /// in it. A source something else writes into (Spotify, a device's AirPlay receiver) is
    /// read whether it is wired or not: a writer with no reader waits, and Spotify would show
    /// a song playing that never plays.
    pub fn keep(&mut self, bay: &Patchbay, wanted: &[SourceId]) {
        let mut wanted: Vec<SourceId> = wanted.to_vec();
        for s in bay.sources() {
            if matches!(s.kind, SourceKind::Spotify | SourceKind::AirPlay)
                && !wanted.contains(&s.id)
            {
                wanted.push(s.id);
            }
        }
        self.open
            .retain(|id, _| wanted.contains(id) && bay.source(*id).is_some());
        for id in &wanted {
            if self.open.contains_key(id) {
                continue;
            }
            let Some(source) = bay.source(*id) else {
                continue;
            };
            let buffer: Shared = Arc::new(Mutex::new(if source.kind == SourceKind::Voice {
                Buffer::spoken()
            } else {
                Buffer::live()
            }));
            let running = match &source.kind {
                SourceKind::Radio(address) => {
                    Running::Station(Station::start(&self.places.ffmpeg, address, buffer.clone()))
                }
                SourceKind::AirPlay | SourceKind::Spotify => {
                    Running::Pipe(Pipe::start(&self.pipe_of(source), buffer.clone()))
                }
                SourceKind::Voice => Running::Voice,
            };
            self.open.insert(*id, (buffer, running));
        }
    }

    /// Takes and lets go of `samples` from every open source the running groups do not play,
    /// so what writes them keeps time even when nothing is wired to them: neither racing
    /// ahead nor waiting for a reader.
    pub fn discard_unheard(&mut self, heard: &[SourceId], samples: usize) {
        for (id, (buffer, _)) in &self.open {
            if !heard.contains(id) {
                let _ = lock(buffer).take(samples);
            }
        }
    }

    /// Hands Endora's speech to the voice source, to be played as it is taken.
    pub fn say(&self, voice: SourceId, samples: &[i16]) {
        if let Some((buffer, Running::Voice)) = self.open.get(&voice) {
            let mut b = lock(buffer);
            b.push(samples);
        }
    }

    /// How much sound waits in each open source's buffer, in milliseconds: the time it spends
    /// between arriving from its receiver or station and being mixed.
    #[must_use]
    pub fn waiting(&self) -> Vec<(SourceId, u32)> {
        self.open
            .iter()
            .map(|(id, (buffer, _))| {
                let samples = lock(buffer).len();
                let ms = samples * 1000 / (RATE as usize * CHANNELS);
                (*id, u32::try_from(ms).unwrap_or(u32::MAX))
            })
            .collect()
    }

    /// The sources open now.
    #[must_use]
    pub fn open(&self) -> Vec<SourceId> {
        self.open.keys().copied().collect()
    }
}

impl Feeds for FeedSet {
    fn take(&mut self, source: SourceId, samples: usize) -> Vec<i16> {
        self.open
            .get(&source)
            .map(|(b, _)| lock(b).take(samples))
            .unwrap_or_default()
    }
}

#[cfg(test)]
#[path = "feeds_tests.rs"]
mod tests;
