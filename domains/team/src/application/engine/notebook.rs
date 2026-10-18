//! Per-member notebooks (ADR 0118): a member adds a note to its own notebook on evidence from
//! HQ's record, forgets one with a reason, reads them all, and says when it has looked them
//! over; the owner may write, edit and delete any member's. A notebook is read in the
//! member's briefing, newest first and capped, with the stale ones marked, and a tidy is asked
//! for after a week. Every rule is code: no tool names another member, so a member cannot
//! write another's notebook.

use serde_json::json;

use super::{Done, Engine, EngineError};
use crate::application::ports::{Clock, Run, TeamStore, WorkerAdapter};
use crate::domain::notebook::{self, Note};
use crate::domain::{EventKind, Member, MemberId, Millis, Ref, Refusal, Role, RunId};

/// The line that ends a briefing, naming the notebook's tools.
pub(super) const TOOLS: &str = "Your notebook: note (on evidence), forget (say why), \
                                read_notebook, notebook_checked.\n";

/// When a member last tidied its notebook (forgot a note, or said it had looked).
fn tidied_key(member: MemberId) -> String {
    format!("notebook_tidied:{}", member.0)
}

/// When a briefing last asked a member to tidy.
fn asked_key(member: MemberId) -> String {
    format!("notebook_tidy_asked:{}", member.0)
}

/// A note as a member names it: `n12`, or a bare 12.
#[must_use]
pub fn note_number(said: &str) -> Option<i64> {
    let said = said.trim();
    said.strip_prefix(['n', 'N'])
        .unwrap_or(said)
        .parse::<i64>()
        .ok()
        .filter(|n| *n > 0)
}

impl<S: TeamStore, W: WorkerAdapter, C: Clock> Engine<S, W, C> {
    /// A member's notebook, oldest first: the owner reads it on the HQ screen.
    ///
    /// # Errors
    /// [`EngineError::Unknown`] for a member that does not exist; a store failure.
    pub fn notebook(&self, member: MemberId) -> Done<Vec<Note>> {
        self.member(member)?;
        Ok(self.store.notes(member)?)
    }

    /// A member's notebook as the HQ screen shows it, oldest first: each note's words,
    /// evidence, who wrote it, and whether it has gone stale.
    ///
    /// # Errors
    /// [`EngineError::Unknown`] for a member that does not exist; a store failure.
    pub fn notebook_shown(&self, member: MemberId) -> Done<serde_json::Value> {
        let now = self.clock.now();
        let notes: Vec<serde_json::Value> = self
            .notebook(member)?
            .iter()
            .map(|n| {
                json!({
                    "id": n.id, "text": n.text,
                    "evidence": n.evidence.iter().map(|r| r.said()).collect::<Vec<_>>(),
                    "from_owner": n.from_owner, "written_at": n.written_at,
                    "written_in_run": n.written_in_run.map(|r| r.0),
                    "last_used_at": n.last_used_at, "stale": n.stale(now),
                })
            })
            .collect();
        Ok(json!({ "member": member.0, "notes": notes }))
    }

    /// A turn adds a note to its own member's notebook: its words (one line, at most 300
    /// characters) and the evidence it rests on, at least one reference, each in HQ's record.
    /// Returns the note's number.
    ///
    /// # Errors
    /// [`Refusal::TurnOver`]; [`Refusal::EmptyNote`], [`Refusal::TooLong`];
    /// [`Refusal::NoteNeedsEvidence`]; [`Refusal::NoSuchEvidence`] naming the missing
    /// reference; [`Refusal::NotebookFull`].
    pub fn note_in(&self, run: RunId, text: &str, cited: &[Ref]) -> Done<i64> {
        let _held = self.held();
        let turn = self.running(run)?;
        let who = self.member(turn.member)?;
        let text = notebook::clean(text).map_err(EngineError::Refused)?;
        if cited.is_empty() {
            return Err(EngineError::Refused(Refusal::NoteNeedsEvidence));
        }
        self.all_there(cited)?;
        if self.store.notes(who.id)?.len() >= notebook::NOTES_MOST {
            return Err(EngineError::Refused(Refusal::NotebookFull));
        }
        let id = self
            .store
            .add_note(who.id, &text, cited, false, Some(run), self.clock.now())?;
        self.emit(
            EventKind::NoteWritten,
            json!({ "member": who.id.0, "name": who.name, "note": id, "text": text,
                    "by": who.name }),
        )?;
        Ok(id)
    }

    /// A turn forgets one of its own member's notes, saying why. Forgetting is a tidy.
    ///
    /// # Errors
    /// [`Refusal::TurnOver`]; [`Refusal::NoSuchNote`] for a note that is not in this member's
    /// notebook, whoever's it is; [`Refusal::ForgetNeedsReason`].
    pub fn forget_in(&self, run: RunId, note: i64, reason: &str) -> Done<Note> {
        let _held = self.held();
        let turn = self.running(run)?;
        let who = self.member(turn.member)?;
        let found = self
            .store
            .notes(who.id)?
            .into_iter()
            .find(|n| n.id == note)
            .ok_or(EngineError::Refused(Refusal::NoSuchNote(note)))?;
        let reason = super::one_line(reason, 300);
        if reason.is_empty() {
            return Err(EngineError::Refused(Refusal::ForgetNeedsReason));
        }
        self.store.delete_note(note)?;
        self.store
            .set_setting(&tidied_key(who.id), &self.clock.now().to_string())?;
        self.emit(
            EventKind::NoteForgotten,
            json!({ "member": who.id.0, "name": who.name, "note": note, "text": found.text,
                    "reason": reason, "by": who.name }),
        )?;
        Ok(found)
    }

    /// A turn reads its own member's whole notebook, newest first, with each note's evidence.
    ///
    /// # Errors
    /// [`Refusal::TurnOver`].
    pub fn read_notebook_in(&self, run: RunId) -> Done<String> {
        let _held = self.held();
        let turn = self.running(run)?;
        let notes = self.store.notes(turn.member)?;
        if notes.is_empty() {
            return Ok("Your notebook is empty.".to_owned());
        }
        let now = self.clock.now();
        let mut said = format!("Your notes ({}), newest first:\n", notes.len());
        for note in notes.iter().rev() {
            let cites = if note.evidence.is_empty() {
                String::new()
            } else {
                format!(" [{}]", Ref::kept(&note.evidence).replace(',', ", "))
            };
            said.push_str(&format!("- {}{cites}\n", note.line(now)));
        }
        Ok(said)
    }

    /// A turn says it has looked over its notebook and nothing needs changing: a tidy.
    ///
    /// # Errors
    /// [`Refusal::TurnOver`].
    pub fn notebook_checked_in(&self, run: RunId) -> Done<()> {
        let _held = self.held();
        let turn = self.running(run)?;
        Ok(self
            .store
            .set_setting(&tidied_key(turn.member), &self.clock.now().to_string())?)
    }

    /// After a post: the member's notes it cited by number, or whose evidence it cited, were
    /// used, so they are not stale.
    pub(super) fn notes_used(&self, run: RunId, text: &str, cited: &[Ref]) -> Done<()> {
        let _held = self.held();
        let Some(turn) = self.store.run(run)? else {
            return Ok(());
        };
        let now = self.clock.now();
        for note in self.store.notes(turn.member)? {
            if note.cited_in(text) || note.rests_on(cited) {
                self.store.touch_note(note.id, now)?;
            }
        }
        Ok(())
    }

    /// The owner adds a note to a member's notebook: no evidence is needed, and it says it is
    /// from the owner. Returns the note's number.
    ///
    /// # Errors
    /// [`EngineError::Unknown`] for a member that does not exist; [`Refusal::EmptyNote`],
    /// [`Refusal::TooLong`], [`Refusal::NotebookFull`].
    pub fn owner_note(&self, member: MemberId, text: &str) -> Done<i64> {
        let _held = self.held();
        let who = self.member(member)?;
        let text = notebook::clean(text).map_err(EngineError::Refused)?;
        if self.store.notes(member)?.len() >= notebook::NOTES_MOST {
            return Err(EngineError::Refused(Refusal::NotebookFull));
        }
        let id = self
            .store
            .add_note(member, &text, &[], true, None, self.clock.now())?;
        self.emit(
            EventKind::NoteWritten,
            json!({ "member": member.0, "name": who.name, "note": id, "text": text,
                    "by": "owner" }),
        )?;
        Ok(id)
    }

    /// The owner rewrites a note in a member's notebook; its evidence stays.
    ///
    /// # Errors
    /// [`Refusal::NoSuchNote`] for a note that is not in this member's notebook;
    /// [`Refusal::EmptyNote`], [`Refusal::TooLong`].
    pub fn owner_edit_note(&self, member: MemberId, note: i64, text: &str) -> Done<()> {
        let _held = self.held();
        let who = self.member(member)?;
        let text = notebook::clean(text).map_err(EngineError::Refused)?;
        self.note_of(member, note)?;
        self.store.set_note_text(note, &text)?;
        self.emit(
            EventKind::NoteWritten,
            json!({ "member": member.0, "name": who.name, "note": note, "text": text,
                    "by": "owner", "edited": true }),
        )
    }

    /// The owner deletes a note from a member's notebook.
    ///
    /// # Errors
    /// [`Refusal::NoSuchNote`] for a note that is not in this member's notebook.
    pub fn owner_delete_note(&self, member: MemberId, note: i64) -> Done<()> {
        let _held = self.held();
        let who = self.member(member)?;
        let found = self.note_of(member, note)?;
        self.store.delete_note(note)?;
        self.emit(
            EventKind::NoteForgotten,
            json!({ "member": member.0, "name": who.name, "note": note, "text": found.text,
                    "reason": "the owner deleted it", "by": "owner" }),
        )
    }

    fn note_of(&self, member: MemberId, note: i64) -> Done<Note> {
        self.store
            .notes(member)?
            .into_iter()
            .find(|n| n.id == note)
            .ok_or(EngineError::Refused(Refusal::NoSuchNote(note)))
    }

    /// The furlough a member is back from, `(from, to)`, if this is its first turn since its
    /// recall.
    pub(super) fn back_from_furlough(
        &self,
        member: &Member,
        run: &Run,
    ) -> Done<Option<(Millis, Millis)>> {
        let (Some(from), Some(to)) = (member.furloughed_at, member.recalled_at) else {
            return Ok(None);
        };
        let since_back = self.store.runs()?.iter().any(|r| {
            r.member == member.id && r.id != run.id && r.started_at.is_some_and(|s| s >= to)
        });
        Ok((!since_back).then_some((from, to)))
    }

    /// The line a briefing opens with for a member back from a furlough: read its notes first.
    pub(super) fn furlough_said(&self, from: Millis, to: Millis) -> String {
        format!(
            "You were furloughed from {} to {}; read your notes first.\n",
            self.said_day(from),
            self.said_day(to)
        )
    }

    /// What a briefing says of a member's notebook: its notes, newest first and capped (the
    /// CEO's only counted); and the ask to tidy them, when it is due (empty otherwise).
    pub(super) fn notebook_said(&self, member: &Member) -> Done<(String, String)> {
        let now = self.clock.now();
        let notes = self.store.notes(member.id)?;
        let said = notebook::briefing(&notes, now, member.role == Role::Ceo);
        let read = |key: String| -> Done<Option<Millis>> {
            Ok(self
                .store
                .setting(&key)?
                .and_then(|said| said.parse::<Millis>().ok()))
        };
        let (tidied, asked) = (read(tidied_key(member.id))?, read(asked_key(member.id))?);
        if !notebook::tidy_due(&notes, tidied, asked, now) {
            return Ok((said, String::new()));
        }
        Ok((said, format!("{}\n", notebook::TIDY_UP)))
    }

    /// The ask to tidy reached the member's briefing: it is not asked again for a day.
    pub(super) fn tidy_asked(&self, member: MemberId) -> Done<()> {
        Ok(self
            .store
            .set_setting(&asked_key(member), &self.clock.now().to_string())?)
    }
}
