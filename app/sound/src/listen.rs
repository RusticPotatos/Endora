//! The live streams the phone, the tablet and the handheld listen to: one MP3 encoder per
//! stream speaker that someone is listening on, fed with whatever that speaker's group plays.
//! The encoder starts with the first listener and stops a little after the last has gone.

use std::collections::BTreeMap;
use std::io::{Read as _, Write as _};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{SyncSender, TrySendError, sync_channel};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use axum::body::Bytes;
use endora_sound::domain::SpeakerId;
use tokio::sync::broadcast;

/// How long an encoder outlives its last listener, so a phone reconnecting does not wait for
/// a new one.
const LINGER: Duration = Duration::from_secs(5);

/// How many blocks may wait for the encoder before new ones are dropped: about a second.
const QUEUED: usize = 50;

/// The arguments `ffmpeg` gets to encode a stream.
#[must_use]
pub fn encoder_arguments() -> Vec<String> {
    [
        "-nostdin",
        "-hide_banner",
        "-loglevel",
        "error",
        "-f",
        "s16le",
        "-ar",
        "44100",
        "-ac",
        "2",
        "-i",
        "pipe:0",
        "-codec:a",
        "libmp3lame",
        "-b:a",
        "192k",
        "-f",
        "mp3",
        "-flush_packets",
        "1",
        "pipe:1",
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect()
}

struct Encoder {
    out: broadcast::Sender<Bytes>,
    pcm: SyncSender<Vec<i16>>,
    child: Child,
    alone_since: Option<Instant>,
}

impl Drop for Encoder {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Every stream someone is listening to.
pub struct Listeners {
    ffmpeg: PathBuf,
    encoders: Mutex<BTreeMap<SpeakerId, Encoder>>,
}

impl Listeners {
    /// No one listening yet.
    #[must_use]
    pub fn new(ffmpeg: PathBuf) -> Self {
        Self {
            ffmpeg,
            encoders: Mutex::new(BTreeMap::new()),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, BTreeMap<SpeakerId, Encoder>> {
        self.encoders.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Listens on `speaker`'s stream, starting its encoder if it has none.
    ///
    /// # Errors
    /// An encoder that cannot be started.
    pub fn listen(&self, speaker: SpeakerId) -> std::io::Result<broadcast::Receiver<Bytes>> {
        let mut encoders = self.lock();
        if let Some(e) = encoders.get_mut(&speaker) {
            e.alone_since = None;
            return Ok(e.out.subscribe());
        }
        let mut child = Command::new(&self.ffmpeg)
            .args(encoder_arguments())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let (mut stdin, mut stdout) = (child.stdin.take(), child.stdout.take());
        let (pcm, blocks) = sync_channel::<Vec<i16>>(QUEUED);
        std::thread::spawn(move || {
            let Some(stdin) = stdin.as_mut() else { return };
            for block in blocks {
                let bytes: Vec<u8> = block.iter().flat_map(|s| s.to_le_bytes()).collect();
                if stdin.write_all(&bytes).is_err() {
                    return;
                }
            }
        });
        let (out, first) = broadcast::channel::<Bytes>(256);
        let sending = out.clone();
        std::thread::spawn(move || {
            let Some(stdout) = stdout.as_mut() else {
                return;
            };
            let mut buf = [0u8; 4096];
            while let Ok(n) = stdout.read(&mut buf) {
                if n == 0 {
                    return;
                }
                // Nobody listening for a moment is not an error: the encoder lingers.
                let _ = sending.send(Bytes::copy_from_slice(&buf[..n]));
            }
        });
        encoders.insert(
            speaker,
            Encoder {
                out,
                pcm,
                child,
                alone_since: None,
            },
        );
        Ok(first)
    }

    /// Hands a block of what `speaker`'s group plays to its encoder, if anyone listens. A
    /// block the encoder cannot take now is dropped: a stream that falls behind skips.
    pub fn feed(&self, speaker: SpeakerId, samples: &[i16]) {
        let encoders = self.lock();
        if let Some(e) = encoders.get(&speaker) {
            match e.pcm.try_send(samples.to_vec()) {
                Ok(()) | Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => {}
            }
        }
    }

    /// The speakers with an encoder running.
    #[must_use]
    pub fn listening(&self) -> Vec<SpeakerId> {
        self.lock().keys().copied().collect()
    }

    /// Stops the encoders nobody has listened to for a while, and forgets any that died, so
    /// the next listener starts a fresh one.
    pub fn tidy(&self) {
        let now = Instant::now();
        self.lock().retain(|_, e| {
            if !matches!(e.child.try_wait(), Ok(None)) {
                return false;
            }
            if e.out.receiver_count() > 0 {
                e.alone_since = None;
                return true;
            }
            let since = *e.alone_since.get_or_insert(now);
            now.duration_since(since) < LINGER
        });
    }
}
