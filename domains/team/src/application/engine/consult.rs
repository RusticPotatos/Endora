//! Asking the past (ADR 0129): `ask_record`, a deterministic search of what the team said that
//! wakes no one, and `consult`, a question to a member that takes one short turn of theirs. A
//! member who is paused or on furlough is never woken: the record answers for them. The cost of
//! a consulted turn counts against the asker's share when both think on a cloud model.

use super::world::day_with_year;
use super::{Author, Done, Engine, EngineError};
use crate::application::ports::{Clock, Run, TeamStore, WorkerAdapter};
use crate::domain::consult::{self, Consult, Status};
use crate::domain::history::HistoryRefusal;
use crate::domain::record::{self, Excerpt, Query};
use crate::domain::{Member, MemberId, Receipt, Ref, Refusal, RunId, TaskId, WakeReason, money};

fn refused(r: HistoryRefusal) -> EngineError {
    EngineError::Refused(Refusal::History(r))
}

/// The tools a consulted turn may use: it reads, and it answers.
const CONSULTED_MAY: [&str; 7] = [
    "my_turn",
    "read_world",
    "read_notebook",
    "read_doc",
    "read_document",
    "ask_record",
    "answer_consult",
];

/// Where HQ counts the calls a consulted turn has made.
fn calls_key(run: RunId) -> String {
    format!("consult_calls:{}", run.0)
}

impl<S: TeamStore, W: WorkerAdapter, C: Clock> Engine<S, W, C> {
    // ---- ask_record -------------------------------------------------------------------------

    /// A member asks the record a question: the passages of what the team said in the last
    /// ninety days that hold its distinctive words, newest first, at most five of at most four
    /// hundred characters, each with something to cite. No one is woken and nothing is spent. A
    /// member on a cloud model is never given a passage from a turn that read Confidential data.
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
        let chosen = record::choose(
            self.store.search_record(&query)?,
            &terms,
            asker.brain.cloud(),
        );
        Ok(self.excerpts_said(&chosen))
    }

    /// The member called `name`, in any case.
    fn named(&self, name: &str) -> Done<Member> {
        self.store
            .members()?
            .into_iter()
            .find(|m| m.name.eq_ignore_ascii_case(name))
            .ok_or_else(|| refused(HistoryRefusal::NoSuchMember(name.to_owned())))
    }

    /// Excerpts as an answer says them, with how to cite them and what they are worth.
    fn excerpts_said(&self, chosen: &[Excerpt]) -> String {
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

    // ---- consult ----------------------------------------------------------------------------

    /// A member consults another: a question, asked directly, answered in the asker's thread as
    /// "<expert> answers (consulted): …", which wakes the asker. The expert takes one short turn,
    /// unless it is paused or on furlough, in which case the record answers for it, now.
    ///
    /// # Errors
    /// [`Refusal::TurnOver`]; [`HistoryRefusal::NoSuchMember`], `NotYourself`, `CeoConsultsLeads`,
    /// `TooManyThisTurn`, `TooManyToday`, `NeedsQuestion`, `NeedsTask`;
    /// [`Refusal::OutOfReach`] for a task outside the turn's reach.
    pub fn consult_in(
        &self,
        run: RunId,
        expert: &str,
        question: &str,
        task: Option<TaskId>,
    ) -> Done<String> {
        let _held = self.held();
        let turn = self.running(run)?;
        let asker = self.member(turn.member)?;
        let expert = self.named(expert.trim())?;
        let question = record::question_of(question);
        if question.is_empty() {
            return Err(refused(HistoryRefusal::NeedsQuestion));
        }
        let thread = match (turn.task, task) {
            (Some(own), _) => own,
            (None, Some(named)) => {
                self.reach(&turn, named, true)?;
                named
            }
            (None, None) => return Err(refused(HistoryRefusal::NeedsTask)),
        };
        let topic = task.or(turn.task);
        if let Some(topic) = topic {
            self.task(topic)?;
        }
        let kept = self.store.consults()?;
        let off = self.settings.offset_minutes;
        let today = self.today();
        let this_turn = kept.iter().filter(|c| c.asker_run == run).count();
        let that_day = kept
            .iter()
            .filter(|c| c.asker == asker.id && money::day_of(c.at, off) == today)
            .count();
        consult::check(&asker, &expert, this_turn, that_day).map_err(refused)?;
        let mut asked = Consult {
            id: 0,
            asker: asker.id,
            asker_run: run,
            expert: expert.id,
            topic,
            thread,
            question,
            status: Status::Queued,
            answer_run: None,
            why: String::new(),
            asker_woken: false,
            at: self.clock.now(),
        };
        asked.id = self.store.add_consult(&asked)?;
        if let Some(why) = consult::cannot_be_woken(&expert) {
            self.answer_from_record(&mut asked, &format!("{why}, so it is not woken"))?;
            return Ok(format!(
                "{} {why} and was not woken. The record answered in {}'s thread, from what they \
                 said: you are woken to read it.",
                expert.name,
                Ref::Task(thread).said()
            ));
        }
        if self.wake_held_lock(expert.id, None, WakeReason::Consulted)? != Receipt::Queued {
            self.answer_from_record(&mut asked, "could not be woken just now")?;
            return Ok(format!(
                "{} could not be woken just now. The record answered in {}'s thread: you are \
                 woken to read it.",
                expert.name,
                Ref::Task(thread).said()
            ));
        }
        Ok(format!(
            "Asked {}. Their answer is posted in the thread of {} and you are woken to read it; \
             end your turn when you have nothing else to do.",
            expert.name,
            Ref::Task(thread).said()
        ))
    }

    /// The consult a turn is the answer to, if it is one.
    pub(super) fn consult_of_run(&self, run: RunId) -> Done<Option<Consult>> {
        Ok(self
            .store
            .consults()?
            .into_iter()
            .find(|c| c.answer_run == Some(run)))
    }

    /// Whether a turn is a consulted one.
    ///
    /// # Errors
    /// A store failure.
    pub fn in_consult(&self, run: RunId) -> Done<bool> {
        Ok(self.consult_of_run(run)?.is_some())
    }

    /// The consult a queued turn of a consulted member answers: the one it already has, or the
    /// oldest asked of the member that no turn has taken.
    fn claim_consult(&self, run: &Run) -> Done<Option<Consult>> {
        if let Some(own) = self.consult_of_run(run.id)? {
            return Ok(Some(own));
        }
        let Some(mut next) = self.store.consults()?.into_iter().find(|c| {
            c.expert == run.member && c.status == Status::Queued && c.answer_run.is_none()
        }) else {
            return Ok(None);
        };
        next.answer_run = Some(run.id);
        self.store.save_consult(&next)?;
        Ok(Some(next))
    }

    /// Who pays for a consulted turn about to start, once it has its consult: the asker when
    /// both think on a cloud model (a consult of a cloud member counts against the asker's
    /// share), otherwise the member consulted. `None` when no consult waits for the turn.
    pub(super) fn consult_payer(&self, run: &Run) -> Done<Option<Member>> {
        let Some(found) = self.claim_consult(run)? else {
            return Ok(None);
        };
        let expert = self.member(found.expert)?;
        let asker = self.member(found.asker)?;
        Ok(Some(if expert.brain.cloud() && asker.brain.cloud() {
            asker
        } else {
            expert
        }))
    }

    /// Whose share a turn's money is booked to: the asker's for a consulted turn of a cloud
    /// member consulted by a cloud member, else the turn's own member's.
    pub(super) fn payer_of(&self, run: &Run) -> Done<MemberId> {
        if run.task.is_some() {
            return Ok(run.member);
        }
        let Some(found) = self.consult_of_run(run.id)? else {
            return Ok(run.member);
        };
        let expert = self.member(found.expert)?;
        let asker = self.member(found.asker)?;
        Ok(if expert.brain.cloud() && asker.brain.cloud() {
            asker.id
        } else {
            run.member
        })
    }

    /// What a consulted member is told: the question under a fixed prefix, its own handover
    /// notes and last words on the task, and what it may do.
    pub(super) fn consult_briefing(&self, member: &Member, found: &Consult) -> Done<String> {
        let asker = self.member(found.asker)?;
        let mut told = format!(
            "You are {}, the team's {}. Today is {}.\n",
            member.name,
            member.role.name(),
            self.said_today()
        );
        told.push_str(&format!(
            "A colleague consults you (ADR 0129). {}\n",
            record::quoted(&asker.name, &found.question, record::QUESTION_MOST)
        ));
        if let Some(topic) = found.topic {
            let on = self.task(topic)?;
            told.push_str(&format!(
                "It is about {} \"{}\".\n",
                Ref::Task(topic).said(),
                record::clean(&on.title)
            ));
            let notes: Vec<String> = self
                .store
                .handovers(topic)?
                .iter()
                .filter(|h| h.by == member.id || h.from == Some(member.id))
                .rev()
                .take(3)
                .map(|h| {
                    record::clean(&format!(
                        "done: {}; not done: {}; next: {}; watch out: {}",
                        h.note.done, h.note.not_done, h.note.next, h.note.watch_out
                    ))
                })
                .collect();
            if !notes.is_empty() {
                told.push_str("Your own handover notes on it, newest first:\n");
                for n in notes {
                    told.push_str(&format!("- {}\n", cut(&n, 600)));
                }
            }
            let words: Vec<String> = self
                .store
                .thread(topic)?
                .iter()
                .filter(|p| p.author == member.name)
                .rev()
                .take(2)
                .map(|p| record::clean(&p.text))
                .collect();
            if !words.is_empty() {
                told.push_str("Your last words in its thread, newest first:\n");
                for w in words {
                    told.push_str(&format!("- {}\n", cut(&w, 500)));
                }
            }
        }
        told.push_str(&format!(
            "Answer in one short turn, from what you know and what the record holds, in a few \
             sentences: read what you need (read_world, read_notebook, read_doc, ask_record; at \
             most {} calls), then answer_consult {{answer}}. Say so if you do not know. Do not \
             start work, post or hand anything on: the answer is posted for {} in the thread of \
             {}.\nYour tools at HQ: my_turn, read_world, read_notebook, read_doc, ask_record, \
             answer_consult.\n",
            consult::ROUNDS,
            asker.name,
            Ref::Task(found.thread).said()
        ));
        Ok(told)
    }

    /// A consulted member answers: the answer is posted in the asker's thread, and the asker is
    /// woken.
    ///
    /// # Errors
    /// [`Refusal::TurnOver`]; [`HistoryRefusal::NoConsultForYou`]; [`HistoryRefusal::NeedsAnswer`].
    pub fn answer_consult_in(&self, run: RunId, answer: &str) -> Done<()> {
        let _held = self.held();
        let turn = self.running(run)?;
        let member = self.member(turn.member)?;
        let Some(mut found) = self
            .consult_of_run(turn.id)?
            .filter(|c| c.status == Status::Queued)
        else {
            return Err(refused(HistoryRefusal::NoConsultForYou));
        };
        let said = record::clean(answer);
        if said.is_empty() {
            return Err(refused(HistoryRefusal::NeedsAnswer));
        }
        let text = format!(
            "{} answers (consulted): {}",
            record::clean(&member.name),
            cut(&said, consult::ANSWER_MOST)
        );
        self.post_held_lock_marked(Author::Member(member.id), found.thread, &text, &[], false)?;
        found.status = Status::Answered;
        self.store.save_consult(&found)?;
        self.wake_the_asker(&mut found)
    }

    /// The record answers a consult the member could not, or did not: what they said about it
    /// in the last ninety days, as testimony, posted in the asker's thread by HQ.
    fn answer_from_record(&self, found: &mut Consult, why: &str) -> Done<()> {
        let expert = self.member(found.expert)?;
        let asker = self.member(found.asker)?;
        let terms = record::terms(&found.question);
        let query = Query {
            terms: terms.clone(),
            since: record::since(self.clock.now()),
            member: Some(expert.name.clone()),
            task: None,
            project: None,
        };
        let chosen = if terms.is_empty() {
            Vec::new()
        } else {
            record::choose(
                self.store.search_record(&query)?,
                &terms,
                asker.brain.cloud(),
            )
        };
        let from = if chosen.is_empty() {
            format!(
                "Nothing {} wrote in the last 90 days holds enough of those words.",
                record::clean(&expert.name)
            )
        } else {
            self.excerpts_said(&chosen)
        };
        let text = format!(
            "Consult of {} ({why}): answered from the record. {from}",
            record::clean(&expert.name)
        );
        self.post_held_lock_marked(Author::Endora, found.thread, text.trim_end(), &[], false)?;
        found.status = Status::FromRecord;
        found.why = why.to_owned();
        self.store.save_consult(found)?;
        self.wake_the_asker(found)
    }

    /// Wakes the asker to read the answer. If its turn is still running the wake would only
    /// join it; the asker is woken when that turn ends.
    fn wake_the_asker(&self, found: &mut Consult) -> Done<()> {
        let receipt = self.wake_held_lock(
            found.asker,
            Some(found.thread),
            WakeReason::QuestionAnswered,
        )?;
        found.asker_woken = receipt != Receipt::Folded;
        self.store.save_consult(found)?;
        Ok(())
    }

    /// What ending a turn means for the consults: a consulted member's turn that ended without
    /// answering is answered from the record; and an asker whose answer came while its turn ran
    /// is woken now.
    pub(super) fn consults_after(&self, run: &Run, consulted: bool) -> Done<()> {
        if consulted {
            let own = match self.consult_of_run(run.id)? {
                Some(own) => Some(own),
                None => self.claim_consult(run)?,
            };
            if let Some(mut own) = own.filter(|c| c.status == Status::Queued) {
                self.answer_from_record(&mut own, "did not answer")?;
            }
        }
        let unread: Vec<Consult> = self
            .store
            .consults()?
            .into_iter()
            .filter(|c| c.asker == run.member && c.status != Status::Queued && !c.asker_woken)
            .collect();
        for mut c in unread {
            self.wake_the_asker(&mut c)?;
        }
        Ok(())
    }

    /// Why a consulted turn may not make this call, if it may not: it reads and it answers, and
    /// it has [`consult::ROUNDS`] calls before it must.
    ///
    /// # Errors
    /// A store failure.
    pub fn consulted_turn_refuses(&self, run: RunId, tool: &str) -> Done<Option<String>> {
        if !self.in_consult(run)? {
            return Ok(None);
        }
        if !CONSULTED_MAY.contains(&tool) {
            return Ok(Some(
                "this turn only answers a consult: read what you need, then answer_consult"
                    .to_owned(),
            ));
        }
        if tool == "answer_consult" {
            return Ok(None);
        }
        let key = calls_key(run);
        let made: usize = self
            .store
            .setting(&key)?
            .and_then(|n| n.parse().ok())
            .unwrap_or(0);
        if made >= consult::ROUNDS {
            return Ok(Some(format!(
                "your consulted turn has used its {} rounds: answer now with answer_consult",
                consult::ROUNDS
            )));
        }
        self.store.set_setting(&key, &(made + 1).to_string())?;
        Ok(None)
    }

    /// The consults as the HQ screen could show them: who asked whom, and how it was answered.
    ///
    /// # Errors
    /// A store failure.
    pub fn consults_shown(&self) -> Done<serde_json::Value> {
        let off = self.settings.offset_minutes;
        let name = |id: MemberId| self.member(id).map(|m| m.name).unwrap_or_default();
        Ok(serde_json::Value::Array(
            self.store
                .consults()?
                .iter()
                .rev()
                .take(50)
                .map(|c| {
                    serde_json::json!({ "id": c.id, "asker": name(c.asker), "expert": name(c.expert),
                        "task": c.thread.0, "question": c.question, "status": c.status.name(),
                        "why": c.why, "at": c.at, "day": day_with_year(c.at, off),
                        "answer_run": c.answer_run.map(|r| r.0) })
                })
                .collect(),
        ))
    }
}

/// `text` cut to `most` characters, with an ellipsis if it was.
fn cut(text: &str, most: usize) -> String {
    if text.chars().count() <= most {
        return text.to_owned();
    }
    let kept: String = text.chars().take(most.saturating_sub(1)).collect();
    format!("{}…", kept.trim_end())
}
