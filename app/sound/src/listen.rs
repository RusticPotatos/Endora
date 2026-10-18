//! The live streams the phone, the tablet and the handheld listen to: one MP3 encoder per
//! stream speaker that someone is listening on, fed with whatever that speaker's group plays.
//! The encoder starts with the first listener and stops a little after the last has gone.

use std::collections::BTreeMap;
use std::io::{Read as _, Write as _};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{Receiver, Sender, SyncSender, TrySendError, channel, sync_channel};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use axum::body::Bytes;
use endora_sound::domain::SpeakerId;
use endora_sound::domain::mix::CHANNELS;
use tokio::sync::broadcast;

use crate::live;

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

/// One speaker's live stream (see [`live`]): the frames going out, and the number of the next.
struct LiveFeed {
    out: broadcast::Sender<Bytes>,
    seq: u32,
}

/// How many 20 ms live frames wait for a slow listener before it skips ahead: a bit over a
/// second. A live stream that falls behind is no longer live.
const LIVE_QUEUED: usize = 64;

/// A probe that cannot start: nobody is listening to that speaker, so nobody would hear it.
#[derive(Debug, PartialEq, Eq)]
pub struct NobodyListens;

/// The hub's clock now, in microseconds since 1970.
pub fn now_us() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_micros()).unwrap_or(u64::MAX))
}

/// Every stream someone is listening to.
pub struct Listeners {
    ffmpeg: PathBuf,
    encoders: Mutex<BTreeMap<SpeakerId, Encoder>>,
    live: Mutex<BTreeMap<SpeakerId, LiveFeed>>,
    probes: Mutex<BTreeMap<SpeakerId, (live::Probe, Sender<u64>)>>,
    intake: Mutex<Vec<(String, u32)>>,
}

impl Listeners {
    /// No one listening yet.
    #[must_use]
    pub fn new(ffmpeg: PathBuf) -> Self {
        Self {
            ffmpeg,
            encoders: Mutex::new(BTreeMap::new()),
            live: Mutex::new(BTreeMap::new()),
            probes: Mutex::new(BTreeMap::new()),
            intake: Mutex::new(Vec::new()),
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

    /// Listens live on `speaker`: its sound as small PCM frames (see [`live`]), with nothing
    /// between the mix and the listener but the frame.
    pub fn listen_live(&self, speaker: SpeakerId) -> broadcast::Receiver<Bytes> {
        let mut live = self.live_lock();
        if let Some(f) = live.get(&speaker) {
            return f.out.subscribe();
        }
        let (out, first) = broadcast::channel::<Bytes>(LIVE_QUEUED);
        live.insert(speaker, LiveFeed { out, seq: 0 });
        first
    }

    fn live_lock(&self) -> std::sync::MutexGuard<'_, BTreeMap<SpeakerId, LiveFeed>> {
        self.live.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Asks for a delay probe in `speaker`'s mix: the next block it is fed has the probe tone
    /// put over it (see [`live::Probe`]). The microsecond, by the hub's clock, at which the
    /// tone began comes on the receiver when it does.
    ///
    /// # Errors
    /// [`NobodyListens`] when no one is listening to the speaker.
    pub fn probe(&self, speaker: SpeakerId) -> Result<Receiver<u64>, NobodyListens> {
        if !self.listening().contains(&speaker) {
            return Err(NobodyListens);
        }
        let (began, told) = channel();
        self.probes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(speaker, (live::Probe::new(), began));
        Ok(told)
    }

    /// Puts a pending probe over the start of `samples`: the changed block, and whether the
    /// tone began in it. `None` when no probe is pending for the speaker.
    fn probed(&self, speaker: SpeakerId, samples: &[i16], at_us: u64) -> Option<(Vec<i16>, bool)> {
        let mut probes = self.probes.lock().unwrap_or_else(PoisonError::into_inner);
        let (probe, told) = probes.get_mut(&speaker)?;
        let mut block = samples.to_vec();
        let began = probe.fresh();
        probe.play_over(&mut block);
        if began {
            let _ = told.send(at_us);
        }
        if probe.finished() {
            probes.remove(&speaker);
        }
        Some((block, began))
    }

    /// Hands a block of what `speaker`'s group plays to its MP3 encoder and to its live
    /// listeners, if there are any. A block the encoder cannot take now is dropped: a stream
    /// that falls behind skips.
    pub fn feed(&self, speaker: SpeakerId, samples: &[i16]) {
        // The hub's clock at the block's first sample: the block was mixed from sound due up
        // to now.
        let at_us = now_us().saturating_sub(live::micros(samples.len() / CHANNELS));
        let probed = self.probed(speaker, samples, at_us);
        let (samples, began) = match &probed {
            Some((block, began)) => (block.as_slice(), *began),
            None => (samples, false),
        };
        if let Some(e) = self.lock().get(&speaker) {
            match e.pcm.try_send(samples.to_vec()) {
                Ok(()) | Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => {}
            }
        }
        let mut live = self.live_lock();
        let Some(f) = live.get_mut(&speaker) else {
            return;
        };
        if f.out.receiver_count() == 0 {
            return;
        }
        for (k, chunk) in live::cut(samples).enumerate() {
            let at = at_us + live::micros(k * live::FRAME_FRAMES);
            let flags = if k == 0 && began { live::FLAG_PROBE } else { 0 };
            // Nobody listening for a moment is not an error: the next frame finds them.
            let _ = f.out.send(live::frame(f.seq, at, flags, chunk));
            f.seq = f.seq.wrapping_add(1);
        }
    }

    /// Notes how much sound waits in each source's buffer, by source name, in milliseconds:
    /// the time it spends between arriving from its receiver and being mixed.
    pub fn note_intake(&self, waiting: Vec<(String, u32)>) {
        *self.intake.lock().unwrap_or_else(PoisonError::into_inner) = waiting;
    }

    /// What [`Listeners::note_intake`] last noted.
    #[must_use]
    pub fn intake(&self) -> Vec<(String, u32)> {
        self.intake
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// The speakers someone is listening to: with an MP3 encoder running, or a live listener.
    #[must_use]
    pub fn listening(&self) -> Vec<SpeakerId> {
        let mut speakers: Vec<SpeakerId> = self.lock().keys().copied().collect();
        speakers.extend(
            self.live_lock()
                .iter()
                .filter(|(_, f)| f.out.receiver_count() > 0)
                .map(|(s, _)| *s),
        );
        speakers.sort_unstable();
        speakers.dedup();
        speakers
    }

    /// Stops the encoders nobody has listened to for a while, and forgets any that died, so
    /// the next listener starts a fresh one.
    pub fn tidy(&self) {
        // A live feed has nothing to linger for: it is only a channel.
        self.live_lock().retain(|_, f| f.out.receiver_count() > 0);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::live::{self, FLAG_PROBE, FRAME_FRAMES};

    fn block(frames: usize, value: i16) -> Vec<i16> {
        vec![value; frames * CHANNELS]
    }

    fn drain(rx: &mut broadcast::Receiver<Bytes>) -> Vec<live::Header> {
        let mut out = Vec::new();
        while let Ok(bytes) = rx.try_recv() {
            out.push(live::parse(&bytes).unwrap().0);
        }
        out
    }

    #[test]
    fn a_live_listener_gets_twenty_millisecond_frames_numbered_in_order_and_timed() {
        let listeners = Listeners::new("ffmpeg".into());
        let mut rx = listeners.listen_live(SpeakerId(4));
        // A block of 50 ms arrives: two whole frames and a short one.
        listeners.feed(SpeakerId(4), &block(FRAME_FRAMES * 5 / 2, 9));
        listeners.feed(SpeakerId(4), &block(FRAME_FRAMES, 8));
        let frames = drain(&mut rx);
        assert_eq!(
            frames.iter().map(|f| f.seq).collect::<Vec<_>>(),
            vec![0, 1, 2, 3]
        );
        assert_eq!(frames[0].samples.len(), FRAME_FRAMES * CHANNELS);
        assert_eq!(frames[2].samples.len(), FRAME_FRAMES * CHANNELS / 2);
        assert!(frames.iter().all(|f| f.flags == 0 && f.rate == 44_100));
        // Within a block each frame is 20 ms after the one before.
        assert_eq!(frames[1].at_us - frames[0].at_us, 20_000);
        assert_eq!(frames[2].at_us - frames[1].at_us, 20_000);
        // The clock is the hub's own wall clock.
        let now = now_us();
        assert!(frames[3].at_us <= now && now - frames[3].at_us < 5_000_000);
    }

    #[test]
    fn a_live_listener_counts_as_listening_until_it_goes() {
        let listeners = Listeners::new("ffmpeg".into());
        assert!(listeners.listening().is_empty());
        let rx = listeners.listen_live(SpeakerId(4));
        assert_eq!(listeners.listening(), vec![SpeakerId(4)]);
        // A second one joins the same feed.
        let rx2 = listeners.listen_live(SpeakerId(4));
        drop(rx);
        assert_eq!(listeners.listening(), vec![SpeakerId(4)]);
        drop(rx2);
        assert!(listeners.listening().is_empty());
        listeners.tidy();
        assert!(listeners.live_lock().is_empty());
    }

    #[test]
    fn a_speaker_nobody_listens_to_sends_no_frames_and_a_slow_listener_skips() {
        let listeners = Listeners::new("ffmpeg".into());
        listeners.feed(SpeakerId(4), &block(FRAME_FRAMES, 1));
        let mut rx = listeners.listen_live(SpeakerId(4));
        for _ in 0..(LIVE_QUEUED + 10) {
            listeners.feed(SpeakerId(4), &block(FRAME_FRAMES, 1));
        }
        // It fell behind by more than the queue holds: it is told, and carries on from the
        // oldest frame still kept, so the sequence it sees jumps.
        assert!(matches!(
            rx.try_recv(),
            Err(broadcast::error::TryRecvError::Lagged(10))
        ));
        let frames = drain(&mut rx);
        assert_eq!(frames.len(), LIVE_QUEUED);
        assert_eq!(frames[0].seq, 10);
    }

    #[test]
    fn a_probe_needs_a_listener() {
        let listeners = Listeners::new("ffmpeg".into());
        assert!(matches!(listeners.probe(SpeakerId(4)), Err(NobodyListens)));
    }

    #[test]
    fn a_probe_replaces_the_mix_with_its_tone_and_flags_the_frame_it_begins_in() {
        let listeners = Listeners::new("ffmpeg".into());
        let mut rx = listeners.listen_live(SpeakerId(4));
        listeners.feed(SpeakerId(4), &block(FRAME_FRAMES, 7_000));
        let began = listeners.probe(SpeakerId(4)).unwrap();
        // Not yet: the tone begins with the next block.
        assert!(began.try_recv().is_err());
        for _ in 0..6 {
            listeners.feed(SpeakerId(4), &block(FRAME_FRAMES, 7_000));
        }
        let at = began.try_recv().unwrap();
        let frames = drain(&mut rx);
        // Seven blocks were fed; the first is before the probe, and is as it was.
        assert_eq!(frames.len(), 7);
        assert!(frames[0].samples.iter().all(|s| *s == 7_000));
        let flagged: Vec<u32> = frames
            .iter()
            .filter(|f| f.flags & FLAG_PROBE != 0)
            .map(|f| f.seq)
            .collect();
        assert_eq!(
            flagged,
            vec![1],
            "only the frame the tone begins in is flagged"
        );
        assert_eq!(
            frames[1].at_us, at,
            "the reply gives the same clock the frame carries"
        );
        assert!(frames[1].samples.iter().all(|s| *s != 7_000));
        // The tone is 80 ms: four frames; the sound after it is the mix again.
        assert!(frames[5].samples.iter().all(|s| *s == 7_000));
        assert!(frames[4].samples.iter().any(|s| *s != 7_000));
        // And it is a one-off: nothing is left pending.
        assert!(listeners.probes.lock().unwrap().is_empty());
    }

    #[test]
    fn a_probe_is_in_the_mp3_too_because_it_is_in_the_mix() {
        // Listeners with no live listener but an encoder-like presence: the probe is applied
        // before either path takes the block, so a listening phone hears it as well.
        let listeners = Listeners::new("ffmpeg".into());
        let _rx = listeners.listen_live(SpeakerId(4));
        let (pcm, blocks) = sync_channel::<Vec<i16>>(8);
        let (out, _) = broadcast::channel::<Bytes>(4);
        let child = Command::new("sleep").arg("5").spawn().unwrap();
        listeners.lock().insert(
            SpeakerId(4),
            Encoder {
                out,
                pcm,
                child,
                alone_since: None,
            },
        );
        let _ = listeners.probe(SpeakerId(4)).unwrap();
        listeners.feed(SpeakerId(4), &block(FRAME_FRAMES, 7_000));
        let sent = blocks.try_recv().unwrap();
        assert!(sent.iter().all(|s| *s != 7_000));
    }

    #[test]
    fn the_sound_waiting_to_be_mixed_is_kept_for_a_probe_to_report() {
        let listeners = Listeners::new("ffmpeg".into());
        assert!(listeners.intake().is_empty());
        listeners.note_intake(vec![("Endora Mixer".to_owned(), 180)]);
        assert_eq!(listeners.intake(), vec![("Endora Mixer".to_owned(), 180)]);
    }
}
