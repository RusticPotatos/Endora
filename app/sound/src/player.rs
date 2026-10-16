//! The player: the one thread that keeps time. Every few milliseconds it brings the engine in
//! line with the patchbay if it changed, mixes whatever is due, and hands each block to its
//! group's sender and to anyone listening on its speakers.

use std::sync::Arc;
use std::time::{Duration, Instant};

use endora_sound::application::engine::{Change, Engine};
use endora_sound::application::hub::Hub;
use endora_sound::domain::SpeakerId;
use endora_sound::domain::mix::{CHANNELS, RATE};
use endora_sound::infrastructure::{FeedSet, FileStore, Places};

use crate::listen::Listeners;

/// How often the player wakes.
const TICK: Duration = Duration::from_millis(20);

/// The most it mixes at once, in frames. After a stall it skips ahead rather than racing to
/// catch up.
const MOST_AT_ONCE: u64 = RATE as u64 / 5;

/// Where a group's sound goes besides the streams: the AirPlay senders.
pub trait Senders: Send {
    /// Does what the engine says the senders must, in order.
    fn apply(&mut self, changes: &[Change]);
    /// Plays a block on `sender`. A group with no sound lately says so, and its sender stops,
    /// leaving its speakers free.
    fn play(&mut self, sender: usize, samples: &[i16], sounding: bool);
}

/// No senders: what runs until the AirPlay senders are set up. It says what it would do.
pub struct NoSenders;

impl Senders for NoSenders {
    fn apply(&mut self, changes: &[Change]) {
        for change in changes {
            println!("sound: no AirPlay senders yet; would {change:?}");
        }
    }

    fn play(&mut self, _sender: usize, _samples: &[i16], _sounding: bool) {}
}

/// Starts the player on a thread of its own.
pub fn start(
    hub: Arc<Hub<FileStore>>,
    listeners: Arc<Listeners>,
    places: Places,
    mut senders: Box<dyn Senders>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut engine = Engine::default();
        let mut feeds = FeedSet::new(places);
        let mut revision = None;
        let began = Instant::now();
        let mut done: u64 = 0;
        let mut tidied = Instant::now();
        loop {
            std::thread::sleep(TICK);
            let now = hub.read(endora_sound::domain::Patchbay::revision);
            if revision != Some(now) {
                let changes = hub.read(|bay| {
                    let changes = engine.plan(bay);
                    let mut wanted: Vec<_> = engine
                        .running()
                        .iter()
                        .flat_map(|r| r.mix.iter().map(|m| m.source))
                        .collect();
                    wanted.sort_unstable();
                    wanted.dedup();
                    feeds.keep(bay, &wanted);
                    changes
                });
                senders.apply(&changes);
                revision = Some(now);
            }
            let due_total = (began.elapsed().as_secs_f64() * f64::from(RATE)) as u64;
            let mut due = due_total.saturating_sub(done);
            if due > MOST_AT_ONCE {
                done += due - MOST_AT_ONCE;
                due = MOST_AT_ONCE;
            }
            if due == 0 {
                continue;
            }
            done += due;
            let frames = usize::try_from(due).unwrap_or(0);
            let played = engine.tick(frames, &mut feeds);
            let mut fed: Vec<SpeakerId> = Vec::new();
            for p in &played {
                if let Some(sender) = p.sender {
                    senders.play(sender, &p.samples, p.sounding);
                }
                for speaker in &p.speakers {
                    listeners.feed(*speaker, &p.samples);
                    fed.push(*speaker);
                }
            }
            // A phone listening on a speaker with nothing wired still hears something: quiet,
            // so its stream stays open for when something is.
            let quiet = vec![0i16; frames * CHANNELS];
            for speaker in listeners.listening() {
                if !fed.contains(&speaker) {
                    listeners.feed(speaker, &quiet);
                }
            }
            if tidied.elapsed() > Duration::from_secs(1) {
                listeners.tidy();
                tidied = Instant::now();
            }
        }
    })
}
