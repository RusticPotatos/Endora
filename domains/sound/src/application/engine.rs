//! The engine: what plays where, now. It turns the patchbay's groups into running groups,
//! each with its own mixer and (when it has AirPlay speakers) its own sender, and mixes each
//! group's next block from its sources.
//!
//! A running group keeps who it is across changes: moving a level, or adding a speaker,
//! changes its mix or its speakers but not its sender, so nothing that is already playing
//! stops to start again. Which group is which is decided by the speakers they share.
//!
//! No threads and no clock here: the caller says how many frames are due and hands over the
//! sources' sound; what it does with each block (a sender's pipe, a phone's stream) is its own.

use std::collections::{BTreeMap, BTreeSet};

use crate::domain::mix::{CHANNELS, Input, Mixer, RATE};
use crate::domain::patchbay::SENDERS;
use crate::domain::{Level, Patchbay, SourceId, SourceKind, SpeakerId, SpeakerKind};

/// How long a group plays nothing before it lets its speakers go: ten seconds.
pub const IDLE_AFTER_FRAMES: u64 = 10 * RATE as u64;

/// A sample quieter than this is silence: a receiver may write silence while the phone is
/// paused, and that must not hold the speakers.
const SOUND_FLOOR: u16 = 8;

/// The sources' sound, as it has arrived.
pub trait Feeds {
    /// Up to `samples` interleaved samples from `source`, fewer if fewer have arrived, none if
    /// it is not playing.
    fn take(&mut self, source: SourceId, samples: usize) -> Vec<i16>;
}

/// One source in a running group's mix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MixIn {
    /// The source.
    pub source: SourceId,
    /// Its wire's level.
    pub level: Level,
    /// Whether it is Endora's voice, which ducks the rest.
    pub voice: bool,
}

/// A group that is playing.
#[derive(Debug, Clone)]
pub struct Running {
    /// Who it is, for as long as it plays.
    pub run: u64,
    /// Its speakers, in id order.
    pub speakers: Vec<SpeakerId>,
    /// Those of them a sender plays, in id order.
    pub airplay: Vec<SpeakerId>,
    /// What it mixes.
    pub mix: Vec<MixIn>,
    /// The sender playing it, if it has AirPlay speakers.
    pub sender: Option<usize>,
    mixer: Mixer,
    last_sound: Option<u64>,
}

/// What the senders must do, in the order given: a speaker is always let go by one sender
/// before another takes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// The sender now plays exactly these speakers.
    Sender {
        /// Which sender.
        sender: usize,
        /// Its speakers, in id order.
        speakers: Vec<SpeakerId>,
    },
    /// The sender plays nothing.
    Released {
        /// Which sender.
        sender: usize,
    },
}

/// One group's next block.
#[derive(Debug, Clone)]
pub struct Played {
    /// The running group.
    pub run: u64,
    /// Its sender, if it has one.
    pub sender: Option<usize>,
    /// Every speaker it plays on, streams included.
    pub speakers: Vec<SpeakerId>,
    /// The mixed block, interleaved.
    pub samples: Vec<i16>,
    /// Whether it has had sound lately. A group that has not lets its sender stop, so its
    /// speakers are free for anything else.
    pub sounding: bool,
}

/// The engine.
#[derive(Debug, Default)]
pub struct Engine {
    running: Vec<Running>,
    next_run: u64,
    now: u64,
}

impl Engine {
    /// The groups playing now.
    #[must_use]
    pub fn running(&self) -> &[Running] {
        &self.running
    }

    /// Brings the running groups in line with the patchbay, and says what the senders must
    /// do. A change that touches no speaker (a level moved) says nothing.
    pub fn plan(&mut self, bay: &Patchbay) -> Vec<Change> {
        let groups = bay.groups();
        let airplay_of = |speakers: &[SpeakerId]| -> Vec<SpeakerId> {
            speakers
                .iter()
                .copied()
                .filter(|s| {
                    bay.speaker(*s)
                        .is_some_and(|sp| sp.kind == SpeakerKind::AirPlay)
                })
                .collect()
        };
        let mix_of = |mix: &[(SourceId, Level)]| -> Vec<MixIn> {
            mix.iter()
                .map(|&(source, level)| MixIn {
                    source,
                    level,
                    voice: bay
                        .source(source)
                        .is_some_and(|s| s.kind == SourceKind::Voice),
                })
                .collect()
        };

        // Which new group is which running one: the most speakers in common first, then the
        // oldest running group, then the first new one.
        let mut pairs: Vec<(usize, u64, usize, usize)> = Vec::new();
        for (r, run) in self.running.iter().enumerate() {
            for (g, group) in groups.iter().enumerate() {
                let shared = group
                    .speakers
                    .iter()
                    .filter(|s| run.speakers.contains(s))
                    .count();
                if shared > 0 {
                    pairs.push((shared, run.run, g, r));
                }
            }
        }
        pairs.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
        let mut group_of_run: BTreeMap<usize, usize> = BTreeMap::new();
        let mut taken_groups: BTreeSet<usize> = BTreeSet::new();
        for (_, _, g, r) in pairs {
            if group_of_run.contains_key(&r) || taken_groups.contains(&g) {
                continue;
            }
            group_of_run.insert(r, g);
            taken_groups.insert(g);
        }

        let before: BTreeMap<usize, Vec<SpeakerId>> = self
            .running
            .iter()
            .filter_map(|r| r.sender.map(|s| (s, r.airplay.clone())))
            .collect();

        // The groups that stay, with what they are now; those that go, gone.
        let mut next: Vec<Running> = Vec::new();
        for (r, mut run) in std::mem::take(&mut self.running).into_iter().enumerate() {
            let Some(&g) = group_of_run.get(&r) else {
                continue;
            };
            run.speakers.clone_from(&groups[g].speakers);
            run.airplay = airplay_of(&groups[g].speakers);
            run.mix = mix_of(&groups[g].mix);
            if run.airplay.is_empty() {
                run.sender = None;
            }
            next.push(run);
        }
        for (g, group) in groups.iter().enumerate() {
            if taken_groups.contains(&g) {
                continue;
            }
            self.next_run += 1;
            next.push(Running {
                run: self.next_run,
                speakers: group.speakers.clone(),
                airplay: airplay_of(&group.speakers),
                mix: mix_of(&group.mix),
                sender: None,
                mixer: Mixer::new(),
                last_sound: None,
            });
        }
        // Senders for the groups that need one and have none: the lowest free.
        let mut free: BTreeSet<usize> = (0..SENDERS).collect();
        for run in &next {
            if let Some(s) = run.sender {
                free.remove(&s);
            }
        }
        for run in &mut next {
            if run.sender.is_none() && !run.airplay.is_empty() {
                run.sender = free.pop_first();
            }
        }
        next.sort_by_key(|r| r.run);
        self.running = next;

        let after: BTreeMap<usize, Vec<SpeakerId>> = self
            .running
            .iter()
            .filter_map(|r| r.sender.map(|s| (s, r.airplay.clone())))
            .collect();
        // First every sender lets go of what it no longer plays; only then does any take on
        // more, so no speaker is ever asked for by two at once.
        let mut changes = Vec::new();
        for (sender, old) in &before {
            match after.get(sender) {
                None => changes.push(Change::Released { sender: *sender }),
                Some(new) => {
                    let kept: Vec<SpeakerId> =
                        old.iter().copied().filter(|s| new.contains(s)).collect();
                    if &kept != old {
                        changes.push(Change::Sender {
                            sender: *sender,
                            speakers: kept,
                        });
                    }
                }
            }
        }
        for (sender, new) in &after {
            let kept: Vec<SpeakerId> = before
                .get(sender)
                .map(|old| old.iter().copied().filter(|s| new.contains(s)).collect())
                .unwrap_or_default();
            if &kept != new {
                changes.push(Change::Sender {
                    sender: *sender,
                    speakers: new.clone(),
                });
            }
        }
        changes
    }

    /// Mixes the next `frames` frames of every running group. Each source is read once, however
    /// many groups it plays in.
    pub fn tick(&mut self, frames: usize, feeds: &mut dyn Feeds) -> Vec<Played> {
        let samples = frames * CHANNELS;
        let mut sound: BTreeMap<SourceId, Vec<i16>> = BTreeMap::new();
        for run in &self.running {
            for m in &run.mix {
                sound
                    .entry(m.source)
                    .or_insert_with(|| feeds.take(m.source, samples));
            }
        }
        let now = self.now;
        let mut played = Vec::with_capacity(self.running.len());
        for run in &mut self.running {
            let heard = run.mix.iter().any(|m| {
                m.level != Level::SILENT
                    && sound[&m.source]
                        .iter()
                        .any(|s| s.unsigned_abs() > SOUND_FLOOR)
            });
            if heard {
                run.last_sound = Some(now);
            }
            let inputs: Vec<Input<'_>> = run
                .mix
                .iter()
                .map(|m| Input {
                    samples: &sound[&m.source],
                    level: m.level,
                    voice: m.voice,
                })
                .collect();
            let mut out = vec![0; samples];
            run.mixer.mix(&inputs, &mut out);
            played.push(Played {
                run: run.run,
                sender: run.sender,
                speakers: run.speakers.clone(),
                samples: out,
                sounding: run
                    .last_sound
                    .is_some_and(|t| now + frames as u64 - t <= IDLE_AFTER_FRAMES),
            });
        }
        self.now += frames as u64;
        played
    }
}
