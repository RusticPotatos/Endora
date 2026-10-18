//! The patchbay: sources, speakers and the wires between them, each wire with its own level,
//! and the groups the wires make. Pure rules; nothing here plays a sound.

use core::fmt;
use std::collections::{BTreeMap, BTreeSet};

/// How many AirPlay senders the hub runs: four stock OwnTone instances (ADR 0104). Each
/// plays one group, so no more than this many groups can include an AirPlay speaker.
pub const SENDERS: usize = 4;

/// The name of the general AirPlay door: the receiver of no device, that any device (an Apple
/// TV, a phone) casts into and that can be wired to any speaker.
pub const GENERAL_DOOR: &str = "Endora Mixer";

/// The longest name a source, speaker or setup may have.
pub const LONGEST_NAME: usize = 40;

/// The longest radio address the hub keeps.
pub const LONGEST_ADDRESS: usize = 500;

macro_rules! id {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(pub u32);

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}", self.0)
            }
        }
    };
}

id!(
    /// Something that makes sound.
    SourceId
);
id!(
    /// Something that plays sound.
    SpeakerId
);

/// A level from 0 to 100: a wire's share of a mix, or a speaker's volume.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Level(u8);

impl Level {
    /// Silence.
    pub const SILENT: Self = Self(0);
    /// All of it.
    pub const FULL: Self = Self(100);

    /// The level `n`, if it is 100 or less.
    #[must_use]
    pub const fn new(n: u8) -> Option<Self> {
        if n > 100 {
            return None;
        }
        Some(Self(n))
    }

    /// The level as a number from 0 to 100.
    #[must_use]
    pub const fn get(self) -> u8 {
        self.0
    }
}

/// What kind of thing a source is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceKind {
    /// A device playing in over AirPlay, through its own receiver.
    AirPlay,
    /// Spotify Connect.
    Spotify,
    /// An internet radio station, at this web address.
    Radio(String),
    /// Endora speaking. It ducks every other wire while it talks.
    Voice,
}

impl SourceKind {
    /// Stable lowercase name, for storage and the API.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::AirPlay => "airplay",
            Self::Spotify => "spotify",
            Self::Radio(_) => "radio",
            Self::Voice => "voice",
        }
    }
}

/// What kind of thing a speaker is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpeakerKind {
    /// An AirPlay 2 speaker, played in sync by one of the senders.
    AirPlay,
    /// A device listening to the hub's live stream (the phone, the tablet, the handheld).
    Stream,
}

impl SpeakerKind {
    /// Stable lowercase name, for storage and the API.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::AirPlay => "airplay",
            Self::Stream => "stream",
        }
    }

    /// The kind with this [`name`](Self::name), if there is one.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "airplay" => Some(Self::AirPlay),
            "stream" => Some(Self::Stream),
            _ => None,
        }
    }
}

/// Something that makes sound.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    /// Its id.
    pub id: SourceId,
    /// What the owner calls it.
    pub name: String,
    /// What it is.
    pub kind: SourceKind,
    /// The device it comes from, if it comes from one. A device's source is never wired to
    /// the same device's speaker.
    pub device: Option<String>,
}

/// Something that plays sound.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Speaker {
    /// Its id.
    pub id: SpeakerId,
    /// What the owner calls it.
    pub name: String,
    /// What it is.
    pub kind: SpeakerKind,
    /// The device it is, if it is one of the owner's own devices.
    pub device: Option<String>,
    /// How loud it plays its mix. Not part of the mix: the senders and the stream players
    /// apply it.
    pub volume: Level,
    /// Another device is playing to it natively. Its wires are kept; it plays none of them
    /// until it is free.
    pub borrowed: bool,
}

/// One source to one speaker, at a level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Wire {
    /// From.
    pub source: SourceId,
    /// To.
    pub speaker: SpeakerId,
    /// Its share of the speaker's mix.
    pub level: Level,
}

/// Speakers playing the same mix, in sync, from one stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    /// Its speakers, in id order.
    pub speakers: Vec<SpeakerId>,
    /// Its sources and their levels, in source id order.
    pub mix: Vec<(SourceId, Level)>,
    /// Whether any of its speakers is AirPlay, so the group needs a sender.
    pub needs_sender: bool,
}

/// Why the patchbay refused a change. A refusal changes nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// There is no such source.
    NoSuchSource,
    /// There is no such speaker.
    NoSuchSpeaker,
    /// There is no setup by that name.
    NoSuchSetup,
    /// A device's own source cannot play on that device: it would hear itself, late.
    ItsOwnSpeaker,
    /// Every sender is playing a group already.
    NoFreeSender,
    /// A name must be 1 to 40 characters once trimmed.
    BadName,
    /// Something else already has that name.
    NameTaken,
    /// A radio station must be an `http` or `https` address.
    NotAWebAddress,
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NoSuchSource => "there is no such source",
            Self::NoSuchSpeaker => "there is no such speaker",
            Self::NoSuchSetup => "there is no setup by that name",
            Self::ItsOwnSpeaker => "a device's own sound cannot play on that device",
            Self::NoFreeSender => "every AirPlay sender is busy with another mix",
            Self::BadName => "a name must be 1 to 40 characters",
            Self::NameTaken => "something already has that name",
            Self::NotAWebAddress => "a radio station must be an http or https address",
        })
    }
}

/// The whole of what the hub plays where. Every change either applies whole and moves the
/// revision on, or is refused and changes nothing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Patchbay {
    sources: BTreeMap<SourceId, Source>,
    speakers: BTreeMap<SpeakerId, Speaker>,
    wires: BTreeMap<(SpeakerId, SourceId), Level>,
    /// Named sets of wires, by the name's lowercase form.
    setups: BTreeMap<String, (String, Vec<Wire>)>,
    next_id: u32,
    revision: u64,
}

/// A name, trimmed, if it is a good one.
fn good_name(name: &str) -> Result<String, Refusal> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > LONGEST_NAME {
        return Err(Refusal::BadName);
    }
    Ok(name.to_owned())
}

/// Whether `address` is a plain web address. The station is fetched by `ffmpeg`, which
/// would as happily read a local file or join several, so nothing else is let through.
#[must_use]
pub fn web_address(address: &str) -> bool {
    let Some(rest) = address
        .strip_prefix("https://")
        .or_else(|| address.strip_prefix("http://"))
    else {
        return false;
    };
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    address.len() <= LONGEST_ADDRESS
        && !host.is_empty()
        && !address.chars().any(|c| c.is_whitespace() || c.is_control())
}

impl Patchbay {
    /// How many changes the patchbay has had. Whatever plays the sound watches it.
    #[must_use]
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    fn changed(&mut self) {
        self.revision += 1;
    }

    fn take_id(&mut self) -> u32 {
        self.next_id += 1;
        self.next_id
    }

    fn name_taken(&self, name: &str) -> bool {
        let name = name.to_lowercase();
        self.sources.values().any(|s| s.name.to_lowercase() == name)
            || self
                .speakers
                .values()
                .any(|s| s.name.to_lowercase() == name)
    }

    /// Adds a source.
    ///
    /// # Errors
    /// A bad or taken name, or a radio station that is not a web address.
    pub fn add_source(
        &mut self,
        name: &str,
        kind: SourceKind,
        device: Option<&str>,
    ) -> Result<SourceId, Refusal> {
        let name = good_name(name)?;
        if self.name_taken(&name) {
            return Err(Refusal::NameTaken);
        }
        if matches!(&kind, SourceKind::Radio(address) if !web_address(address)) {
            return Err(Refusal::NotAWebAddress);
        }
        let id = SourceId(self.take_id());
        let device = device
            .map(|d| d.trim().to_lowercase())
            .filter(|d| !d.is_empty());
        self.sources.insert(
            id,
            Source {
                id,
                name,
                kind,
                device,
            },
        );
        self.changed();
        Ok(id)
    }

    /// Adds a speaker, at full volume.
    ///
    /// # Errors
    /// A bad or taken name.
    pub fn add_speaker(
        &mut self,
        name: &str,
        kind: SpeakerKind,
        device: Option<&str>,
    ) -> Result<SpeakerId, Refusal> {
        let name = good_name(name)?;
        if self.name_taken(&name) {
            return Err(Refusal::NameTaken);
        }
        let id = SpeakerId(self.take_id());
        let device = device
            .map(|d| d.trim().to_lowercase())
            .filter(|d| !d.is_empty());
        self.speakers.insert(
            id,
            Speaker {
                id,
                name,
                kind,
                device,
                volume: Level::FULL,
                borrowed: false,
            },
        );
        self.changed();
        Ok(id)
    }

    /// Removes a source and its wires. Whether there was one.
    pub fn remove_source(&mut self, id: SourceId) -> bool {
        if self.sources.remove(&id).is_none() {
            return false;
        }
        self.wires.retain(|(_, source), _| *source != id);
        self.changed();
        true
    }

    /// Removes a speaker and its wires. Whether there was one.
    pub fn remove_speaker(&mut self, id: SpeakerId) -> bool {
        if self.speakers.remove(&id).is_none() {
            return false;
        }
        self.wires.retain(|(speaker, _), _| *speaker != id);
        self.changed();
        true
    }

    /// Moves the old per-device AirPlay doors ("Endora Mixer (Pad)") into the general door:
    /// each of their wires (and those in saved setups) goes to the general door's same
    /// speaker, at the higher level when two meet, and the device doors are removed. The
    /// general door is made if need be. How many device doors it removed.
    ///
    /// # Errors
    /// Something other than a door has the general door's name.
    pub fn merge_device_doors(&mut self) -> Result<usize, Refusal> {
        let old: Vec<SourceId> = self
            .sources
            .values()
            .filter(|s| s.kind == SourceKind::AirPlay && s.device.is_some())
            .map(|s| s.id)
            .collect();
        if old.is_empty() {
            return Ok(0);
        }
        let general = self.ensure_general_door()?;
        let moved: Vec<((SpeakerId, SourceId), Level)> = self
            .wires
            .iter()
            .filter(|((_, source), _)| old.contains(source))
            .map(|(k, l)| (*k, *l))
            .collect();
        for ((speaker, source), level) in moved {
            self.wires.remove(&(speaker, source));
            let best = self
                .wires
                .get(&(speaker, general))
                .map_or(level, |now| level.max(*now));
            self.wires.insert((speaker, general), best);
        }
        for (_, wires) in self.setups.values_mut() {
            let mut merged: BTreeMap<SpeakerId, Wire> = BTreeMap::new();
            let mut kept: Vec<Wire> = Vec::new();
            for w in wires.iter() {
                if !old.contains(&w.source) {
                    kept.push(*w);
                    continue;
                }
                let at = merged.entry(w.speaker).or_insert(Wire {
                    source: general,
                    speaker: w.speaker,
                    level: w.level,
                });
                at.level = at.level.max(w.level);
            }
            for (_, w) in merged {
                match kept
                    .iter_mut()
                    .find(|k| k.source == general && k.speaker == w.speaker)
                {
                    Some(k) => k.level = k.level.max(w.level),
                    None => kept.push(w),
                }
            }
            *wires = kept;
        }
        for id in &old {
            self.sources.remove(id);
        }
        self.changed();
        Ok(old.len())
    }

    /// The general AirPlay door (see [`GENERAL_DOOR`]), made if there is none. Any device can
    /// cast into it, and it can be wired to any speaker, since it belongs to no device.
    ///
    /// # Errors
    /// Something else already has its name.
    pub fn ensure_general_door(&mut self) -> Result<SourceId, Refusal> {
        if let Some(door) = self
            .sources
            .values()
            .find(|s| s.kind == SourceKind::AirPlay && s.device.is_none())
        {
            return Ok(door.id);
        }
        self.add_source(GENERAL_DOOR, SourceKind::AirPlay, None)
    }

    /// The source with this id.
    #[must_use]
    pub fn source(&self, id: SourceId) -> Option<&Source> {
        self.sources.get(&id)
    }

    /// The speaker with this id.
    #[must_use]
    pub fn speaker(&self, id: SpeakerId) -> Option<&Speaker> {
        self.speakers.get(&id)
    }

    /// Every source, in id order.
    #[must_use]
    pub fn sources(&self) -> Vec<&Source> {
        self.sources.values().collect()
    }

    /// Every speaker, in id order.
    #[must_use]
    pub fn speakers(&self) -> Vec<&Speaker> {
        self.speakers.values().collect()
    }

    /// Every wire, by speaker then source.
    #[must_use]
    pub fn wires(&self) -> Vec<Wire> {
        self.wires
            .iter()
            .map(|(&(speaker, source), &level)| Wire {
                source,
                speaker,
                level,
            })
            .collect()
    }

    /// Joins `source` to `speaker` at `level`, or sets the level of the wire already there.
    ///
    /// # Errors
    /// An unknown source or speaker, a device's source to its own speaker, or a change that
    /// would need more senders than there are.
    pub fn wire(
        &mut self,
        source: SourceId,
        speaker: SpeakerId,
        level: Level,
    ) -> Result<(), Refusal> {
        let from = self.sources.get(&source).ok_or(Refusal::NoSuchSource)?;
        let to = self.speakers.get(&speaker).ok_or(Refusal::NoSuchSpeaker)?;
        if from.device.is_some() && from.device == to.device {
            return Err(Refusal::ItsOwnSpeaker);
        }
        let mut next = self.wires.clone();
        next.insert((speaker, source), level);
        self.within_senders(&next, &self.speakers)?;
        self.wires = next;
        self.changed();
        Ok(())
    }

    /// Takes the wire from `source` to `speaker` off. Whether there was one.
    pub fn unwire(&mut self, source: SourceId, speaker: SpeakerId) -> bool {
        if self.wires.remove(&(speaker, source)).is_none() {
            return false;
        }
        self.changed();
        true
    }

    /// Sets every wire `source` already has to `level`, and says how many that was. Adds no
    /// wire; each stays adjustable on its own afterwards.
    ///
    /// # Errors
    /// An unknown source, or a change that would need more senders than there are.
    pub fn set_all(&mut self, source: SourceId, level: Level) -> Result<usize, Refusal> {
        if !self.sources.contains_key(&source) {
            return Err(Refusal::NoSuchSource);
        }
        let mut next = self.wires.clone();
        let mut count = 0;
        for ((_, from), l) in &mut next {
            if *from == source {
                *l = level;
                count += 1;
            }
        }
        self.within_senders(&next, &self.speakers)?;
        self.wires = next;
        self.changed();
        Ok(count)
    }

    /// Sets how loud a speaker plays.
    ///
    /// # Errors
    /// An unknown speaker.
    pub fn set_volume(&mut self, speaker: SpeakerId, volume: Level) -> Result<(), Refusal> {
        let to = self
            .speakers
            .get_mut(&speaker)
            .ok_or(Refusal::NoSuchSpeaker)?;
        to.volume = volume;
        self.changed();
        Ok(())
    }

    /// Marks a speaker borrowed by another device, or free again. A borrowed speaker keeps
    /// its wires and plays none of them.
    ///
    /// # Errors
    /// An unknown speaker, or a freed speaker whose wires would need a sender there is not:
    /// it then stays borrowed until one is free.
    pub fn set_borrowed(&mut self, speaker: SpeakerId, borrowed: bool) -> Result<(), Refusal> {
        if !self.speakers.contains_key(&speaker) {
            return Err(Refusal::NoSuchSpeaker);
        }
        let mut next = self.speakers.clone();
        if let Some(s) = next.get_mut(&speaker) {
            s.borrowed = borrowed;
        }
        self.within_senders(&self.wires, &next)?;
        self.speakers = next;
        self.changed();
        Ok(())
    }

    /// Saves the wires as they are now, under `name`, replacing a setup of the same name.
    ///
    /// # Errors
    /// A bad name.
    pub fn save_setup(&mut self, name: &str) -> Result<(), Refusal> {
        let name = good_name(name)?;
        let wires = self.wires();
        self.setups.insert(name.to_lowercase(), (name, wires));
        self.changed();
        Ok(())
    }

    /// Puts a setup's wires back as the only wires. Wires to a source or speaker that has
    /// gone since are skipped, and so are wires a device's own speaker cannot take.
    ///
    /// # Errors
    /// No setup by that name, or wires that would need more senders than there are; then
    /// nothing changes.
    pub fn apply_setup(&mut self, name: &str) -> Result<(), Refusal> {
        let (_, wires) = self
            .setups
            .get(&name.trim().to_lowercase())
            .ok_or(Refusal::NoSuchSetup)?;
        let mut next = BTreeMap::new();
        for w in wires {
            let (Some(from), Some(to)) =
                (self.sources.get(&w.source), self.speakers.get(&w.speaker))
            else {
                continue;
            };
            if from.device.is_some() && from.device == to.device {
                continue;
            }
            next.insert((w.speaker, w.source), w.level);
        }
        self.within_senders(&next, &self.speakers)?;
        self.wires = next;
        self.changed();
        Ok(())
    }

    /// Forgets a setup. Whether there was one.
    pub fn remove_setup(&mut self, name: &str) -> bool {
        if self.setups.remove(&name.trim().to_lowercase()).is_none() {
            return false;
        }
        self.changed();
        true
    }

    /// The wires saved under `name`.
    #[must_use]
    pub fn setup(&self, name: &str) -> Option<&[Wire]> {
        self.setups
            .get(&name.trim().to_lowercase())
            .map(|(_, wires)| wires.as_slice())
    }

    /// Every setup's name as the owner wrote it, with its wires, in name order.
    #[must_use]
    pub fn setups(&self) -> Vec<(&str, &[Wire])> {
        self.setups
            .values()
            .map(|(name, wires)| (name.as_str(), wires.as_slice()))
            .collect()
    }

    /// The groups the wires make: speakers whose wires are the same, at the same levels,
    /// play one mix from one stream. A speaker with no wires, or a borrowed one, is in none.
    /// In order of each group's first speaker.
    #[must_use]
    pub fn groups(&self) -> Vec<Group> {
        groups_of(&self.wires, &self.speakers)
    }

    /// Refuses wires and speakers that would make more groups needing a sender than there
    /// are senders.
    fn within_senders(
        &self,
        wires: &BTreeMap<(SpeakerId, SourceId), Level>,
        speakers: &BTreeMap<SpeakerId, Speaker>,
    ) -> Result<(), Refusal> {
        let needing = groups_of(wires, speakers)
            .iter()
            .filter(|g| g.needs_sender)
            .count();
        if needing > SENDERS {
            return Err(Refusal::NoFreeSender);
        }
        Ok(())
    }

    // ---- restoring a stored patchbay -------------------------------------------------

    /// Puts back a source exactly as it was stored, id and all. For the store only: it
    /// checks nothing, because what it restores was checked when it was made.
    pub fn restore_source(&mut self, source: Source) {
        self.next_id = self.next_id.max(source.id.0);
        self.sources.insert(source.id, source);
    }

    /// Puts back a speaker exactly as it was stored. For the store only.
    pub fn restore_speaker(&mut self, speaker: Speaker) {
        self.next_id = self.next_id.max(speaker.id.0);
        self.speakers.insert(speaker.id, speaker);
    }

    /// Puts back a wire exactly as it was stored. For the store only; a wire to something
    /// that is not there is dropped.
    pub fn restore_wire(&mut self, wire: Wire) {
        if self.sources.contains_key(&wire.source) && self.speakers.contains_key(&wire.speaker) {
            self.wires.insert((wire.speaker, wire.source), wire.level);
        }
    }

    /// Puts back a setup exactly as it was stored. For the store only.
    pub fn restore_setup(&mut self, name: &str, wires: Vec<Wire>) {
        self.setups
            .insert(name.to_lowercase(), (name.to_owned(), wires));
    }

    /// Puts back the revision the store had, so it keeps moving forward across restarts.
    pub fn restore_revision(&mut self, revision: u64) {
        self.revision = revision;
    }

    /// The last id handed out. Kept with the patchbay, so an id is never handed out twice:
    /// a setup may still name a source that has gone, and must not find a new one there.
    #[must_use]
    pub const fn last_id(&self) -> u32 {
        self.next_id
    }

    /// Puts back the last id handed out. For the store only; it never goes backwards.
    pub fn restore_last_id(&mut self, last: u32) {
        self.next_id = self.next_id.max(last);
    }
}

fn groups_of(
    wires: &BTreeMap<(SpeakerId, SourceId), Level>,
    speakers: &BTreeMap<SpeakerId, Speaker>,
) -> Vec<Group> {
    let mut mixes: BTreeMap<SpeakerId, Vec<(SourceId, Level)>> = BTreeMap::new();
    for (&(speaker, source), &level) in wires {
        if speakers.get(&speaker).is_some_and(|s| !s.borrowed) {
            mixes.entry(speaker).or_default().push((source, level));
        }
    }
    let mut groups: Vec<Group> = Vec::new();
    let mut seen: BTreeSet<Vec<(SourceId, Level)>> = BTreeSet::new();
    for (speaker, mix) in &mixes {
        if seen.contains(mix) {
            let group = groups.iter_mut().find(|g| &g.mix == mix);
            if let Some(group) = group {
                group.speakers.push(*speaker);
                group.needs_sender |= speakers[speaker].kind == SpeakerKind::AirPlay;
            }
            continue;
        }
        seen.insert(mix.clone());
        groups.push(Group {
            speakers: vec![*speaker],
            mix: mix.clone(),
            needs_sender: speakers[speaker].kind == SpeakerKind::AirPlay,
        });
    }
    groups
}

#[cfg(test)]
#[path = "patchbay_tests.rs"]
mod tests;
