//! Retract and sweep (ADR 0124): the owner's "Not true" on a note, a section of the world or
//! a story. HQ keeps the claim's fingerprint, takes the piece out, lists what else says the
//! same for the owner to forget at once, and refuses the claim when a member writes it again.
//! A note that cites another (`n12`) is built on it: forgetting one names what was built on it.

use serde_json::{Value, json};

use super::{Done, Engine, EngineError};
use crate::application::ports::{Clock, TeamStore, WorkerAdapter};
use crate::domain::guards::GuardRefusal;
use crate::domain::retraction::{self, Retraction};
use crate::domain::world::{Address, HOUSE, Stage};
use crate::domain::{EventKind, MemberId, Refusal};

/// A piece of the record a retraction can take out: a note, a section, a story.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Piece {
    /// A note, by its number, and whose it is.
    Note(i64, MemberId),
    /// A section, by its address.
    Section(String),
    /// A story, by its address.
    Story(String),
}

fn refused(r: GuardRefusal) -> EngineError {
    EngineError::Refused(Refusal::Guard(r))
}

impl<S: TeamStore, W: WorkerAdapter, C: Clock> Engine<S, W, C> {
    /// Every current piece the owner could retract, with what it says, and the address it is
    /// named by.
    fn retractable(&self) -> Done<Vec<(String, Piece, String, String)>> {
        let mut all = Vec::new();
        for m in self.store.members()? {
            for n in self.store.notes(m.id)? {
                all.push((
                    format!("note:{}", n.id),
                    Piece::Note(n.id, m.id),
                    n.text.clone(),
                    format!("{}'s notebook", m.name),
                ));
            }
        }
        for s in self.store.sections()? {
            if !s.stage.current() {
                continue;
            }
            let address = s.address();
            all.push((
                address.clone(),
                Piece::Section(address),
                format!("{} {} {}", s.heading, s.headline, s.body),
                format!("the {} page", s.page),
            ));
        }
        for s in self.store.stories()? {
            if !s.stage.current() {
                continue;
            }
            let address = s.address();
            all.push((
                address.clone(),
                Piece::Story(address),
                format!("{} {} {} {}", s.moment, s.reflex, s.instead, s.because),
                "a story".to_owned(),
            ));
        }
        Ok(all)
    }

    /// Takes a piece out of what members read: a note forgotten, a section or story retired,
    /// each saying the owner retracted it.
    fn take_out(&self, piece: &Piece, why: &str) -> Done<()> {
        let now = self.clock.now();
        match piece {
            Piece::Note(id, member) => {
                let who = self.member(*member)?;
                let text = self
                    .store
                    .notes(*member)?
                    .into_iter()
                    .find(|n| n.id == *id)
                    .map(|n| n.text)
                    .unwrap_or_default();
                self.store.delete_note(*id)?;
                self.emit(
                    EventKind::NoteForgotten,
                    json!({ "member": member.0, "name": who.name, "note": id, "text": text,
                            "reason": why, "by": "owner" }),
                )
            }
            Piece::Section(address) | Piece::Story(address) => {
                let said = format!("{why} ({})", self.said_day(now));
                let title = match Self::address(address)? {
                    Address::Section { page, anchor } => {
                        let Some(mut s) = self.section_at(&page, &anchor)? else {
                            return Ok(());
                        };
                        s.stage = Stage::Retired;
                        s.why.clone_from(&said);
                        s.revision += 1;
                        s.updated_at = now;
                        s.written_by = "the owner".to_owned();
                        s.written_in_run = None;
                        self.store.save_section(&s, &said)?;
                        s.headline
                    }
                    Address::Story(handle) => {
                        let Some(mut s) = self.story_called(&handle)? else {
                            return Ok(());
                        };
                        s.stage = Stage::Retired;
                        s.why.clone_from(&said);
                        self.store.save_story(&s)?;
                        s.short()
                    }
                    Address::Page(_) => return Ok(()),
                };
                self.timeline_line(
                    HOUSE.slug,
                    "the owner",
                    None,
                    &format!("\"{title}\" ({address}): {said}"),
                )?;
                self.emit(
                    EventKind::WorldChanged,
                    json!({ "what": "retracted", "address": address, "by": "owner", "why": why }),
                )
            }
        }
    }

    /// The owner says a note, section or story is not true (ADR 0124): its fingerprint is
    /// kept, it is taken out, and what else says the same is listed for the owner to forget.
    ///
    /// # Errors
    /// [`GuardRefusal::NotRetractable`] for an address that names none of them.
    pub fn retract(&self, target: &str) -> Done<Value> {
        let _held = self.held();
        let target = target.trim().to_ascii_lowercase();
        let pieces = self.retractable()?;
        let Some((address, piece, text, _)) = pieces.iter().find(|(a, ..)| *a == target).cloned()
        else {
            return Err(refused(GuardRefusal::NotRetractable(target)));
        };
        let now = self.clock.now();
        let id = self.store.add_retraction(&Retraction {
            id: 0,
            target: address.clone(),
            text: text.clone(),
            fingerprint: retraction::fingerprint(&text),
            at: now,
        })?;
        let built_on = match &piece {
            Piece::Note(note, _) => self.store.note_children(*note)?,
            _ => Vec::new(),
        };
        self.take_out(&piece, "Not true: retracted by the owner")?;
        Ok(json!({
            "retraction": id, "target": address, "built_on_it": built_on
                .iter().map(|n| format!("note:{n}")).collect::<Vec<_>>(),
            "sweep": self.swept(id)?,
        }))
    }

    /// What else says what a retraction said, most alike first.
    fn swept(&self, id: i64) -> Done<Vec<Value>> {
        let Some(r) = self.store.retractions()?.into_iter().find(|r| r.id == id) else {
            return Err(EngineError::Unknown("retraction"));
        };
        let mut found: Vec<(usize, Value)> = self
            .retractable()?
            .into_iter()
            .filter(|(address, ..)| *address != r.target)
            .filter_map(|(address, _, text, whose)| {
                let print = retraction::fingerprint(&text);
                retraction::says_again(&r.fingerprint, &print).then(|| {
                    let alike = retraction::likeness(&r.fingerprint, &print).percent;
                    (
                        alike,
                        json!({ "target": address, "text": text, "whose": whose, "alike": alike }),
                    )
                })
            })
            .collect();
        found.sort_by_key(|(alike, _)| core::cmp::Reverse(*alike));
        Ok(found.into_iter().map(|(_, v)| v).collect())
    }

    /// The sweep of a retraction: what else says the same now.
    ///
    /// # Errors
    /// [`EngineError::Unknown`] for a retraction that does not exist.
    pub fn sweep(&self, id: i64) -> Done<Vec<Value>> {
        let _held = self.held();
        self.swept(id)
    }

    /// The owner forgets, at once, what of a retraction's sweep they ticked. Only what the
    /// sweep still finds is taken out. Returns how many were.
    ///
    /// # Errors
    /// [`EngineError::Unknown`] for a retraction that does not exist.
    pub fn forget_swept(&self, id: i64, targets: &[String]) -> Done<usize> {
        let _held = self.held();
        let swept: Vec<String> = self
            .swept(id)?
            .iter()
            .filter_map(|v| v["target"].as_str().map(str::to_owned))
            .collect();
        let pieces = self.retractable()?;
        let mut forgot = 0;
        for target in targets {
            if !swept.contains(target) {
                continue;
            }
            if let Some((_, piece, ..)) = pieces.iter().find(|(a, ..)| a == target) {
                self.take_out(piece, &format!("It says what retraction {id} said"))?;
                forgot += 1;
            }
        }
        Ok(forgot)
    }

    /// Refuses a member's writing that says again what the owner retracted (ADR 0124).
    pub(super) fn not_retracted(&self, text: &str) -> Done<()> {
        let print = retraction::fingerprint(text);
        let Some(r) = self
            .store
            .retractions()?
            .into_iter()
            .find(|r| retraction::says_again(&r.fingerprint, &print))
        else {
            return Ok(());
        };
        Err(refused(GuardRefusal::Retracted {
            target: r.target,
            on: self.said_day(r.at),
        }))
    }

    /// A new note's parents: the notes it cites by number (`n12`), that exist.
    pub(super) fn note_parents(&self, note: i64, text: &str) -> Done<()> {
        let mut known = Vec::new();
        for m in self.store.members()? {
            known.extend(self.store.notes(m.id)?.into_iter().map(|n| n.id));
        }
        for word in text.split(|c: char| !c.is_ascii_alphanumeric()) {
            let Some(rest) = word.strip_prefix(['n', 'N']) else {
                continue;
            };
            let Ok(parent) = rest.parse::<i64>() else {
                continue;
            };
            if parent != note && known.contains(&parent) {
                self.store.add_note_parent(note, parent)?;
            }
        }
        Ok(())
    }

    /// What a forgotten note's descendants are told to its forgetter: the notes built on it.
    pub fn built_on(&self, note: i64) -> Done<String> {
        let children = self.store.note_children(note)?;
        if children.is_empty() {
            return Ok(String::new());
        }
        let named: Vec<String> = children.iter().map(|n| format!("n{n}")).collect();
        Ok(format!(
            " Built on it: {}. Look at whether they still hold.",
            named.join(", ")
        ))
    }
}
