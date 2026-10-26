//! Explicit historical testimony search; no peer scheduling or answer substitution.

use super::world::day_with_year;
use super::{Done, Engine, EngineError};
use crate::application::ports::{Clock, TeamStore, WorkerAdapter};
use crate::domain::history::HistoryRefusal;
use crate::domain::record::{self, Excerpt, Query};
use crate::domain::{EventKind, Ref, Refusal, RunId, TaskId};

fn refused(reason: HistoryRefusal) -> EngineError {
    EngineError::Refused(Refusal::History(reason))
}

impl<S: TeamStore, W: WorkerAdapter, C: Clock> Engine<S, W, C> {
    // ---- ask_record -------------------------------------------------------------------------

    /// A member asks the record a question: the passages of what the team said in the last
    /// ninety days that hold its distinctive words, newest first, at most five of at most four
    /// hundred characters, each with something to cite. No one is woken and nothing is spent. A
    /// member on a cloud model is never given a passage from a turn that read Restricted data
    /// (ADR 0141; before it, Confidential).
    ///
    /// # Errors
    /// [`Refusal::TurnOver`]; [`HistoryRefusal::NothingToSearchFor`]; [`HistoryRefusal::NoSuchMember`].
    pub fn ask_record_in(
        &self,
        run: RunId,
        question: &str,
        member: Option<&str>,
        task: Option<TaskId>,
        project: Option<i64>,
    ) -> Done<String> {
        let _held = self.held();
        let turn = self.running(run)?;
        let asker = self.member(turn.member)?;
        let terms = record::terms(question);
        if terms.is_empty() {
            return Err(refused(HistoryRefusal::NothingToSearchFor));
        }
        let by = match member.map(str::trim).filter(|m| !m.is_empty()) {
            Some(name) => Some(self.named(name)?.name),
            None => None,
        };
        if let Some(task) = task {
            self.task(task)?;
        }
        let query = Query {
            terms: terms.clone(),
            since: record::since(self.clock.now()),
            member: by,
            task,
            project,
        };
        // What a turn is given depends on the brain it runs on (ADR 0131): since ADR 0141 only a
        // Restricted passage is kept from a turn on Claude.
        let on_the_cloud = turn.brain.unwrap_or(asker.brain).cloud();
        let found = self.store.search_record(&query)?;
        let chosen = record::choose(found.clone(), &terms, on_the_cloud);
        // A passage from a turn that read Confidential data still marks the turn given it.
        if chosen.iter().any(|e| {
            found.iter().any(|p| {
                p.locator == e.locator && p.tier >= endora_kernel::tier::Tier::Confidential
            })
        }) {
            // Fail closed before returning any inherited confidential text. This also protects
            // later transcripts and record searches of this turn's posts and handovers.
            self.store.mark_confidential_input(run)?;
        }
        // How much the memory is used is counted from these (ADR 0132).
        self.emit(
            EventKind::RecordAsked,
            serde_json::json!({ "run": run.0, "member": asker.id.0,
                                "task": turn.task.map(|t| t.0), "found": chosen.len() }),
        )?;
        Ok(self.excerpts_said(&chosen))
    }

    /// Excerpts as an answer says them, with how to cite them and what they are worth.
    pub(super) fn excerpts_said(&self, chosen: &[Excerpt]) -> String {
        if chosen.is_empty() {
            return "Nothing in HQ's record of the last 90 days holds enough of those words. \
                    Try other words, or consult a member who was there."
                .to_owned();
        }
        let off = self.settings.offset_minutes;
        let mut said = String::from(
            "From HQ's record, newest first. These are testimony: cite them for what was said, \
             never as a reading of the lab now (take a fresh reading for that).\n",
        );
        for (i, e) in chosen.iter().enumerate() {
            let on = e
                .task
                .map_or(String::new(), |t| format!(", on {}", Ref::Task(t).said()));
            let cite = e.task.map_or_else(
                || format!("find it: {}", e.locator),
                |t| format!("cite: {}; find it: {}", Ref::Task(t).said(), e.locator),
            );
            said.push_str(&format!(
                "{}. {} by {}, {}{on}: \"{}\" ({cite})\n",
                i + 1,
                e.source.label(),
                e.by,
                day_with_year(e.at, off),
                e.text
            ));
        }
        said
    }
}
