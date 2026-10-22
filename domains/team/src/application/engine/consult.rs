//! Asking the past (ADR 0129): `ask_record`, a deterministic search of what the team said that
//! wakes no one, and `consult`, a question to a member that takes one short turn of theirs. A
//! member who is paused or on furlough is never woken: the record answers for them. A consulted
//! turn is work on the house's own model (ADR 0131): it costs nothing, and nobody's share.

use super::world::day_with_year;
use super::{Author, Done, Engine, EngineError};
use crate::application::history_ports::{ConsultDelivery, ConsultPostEffects};
use crate::application::ports::{Clock, Run, TeamStore, WorkerAdapter};
use crate::domain::consult::{self, Consult, Status};
use crate::domain::history::HistoryRefusal;
use crate::domain::record::{self, Excerpt, Query};
use crate::domain::{EventKind, Member, Receipt, Ref, Refusal, RunId, TaskId, WakeReason, money};

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
        // What a turn is given depends on the brain it runs on (ADR 0131): a turn on the house's
        // model may be given a passage from a turn that read Confidential data, a turn on Claude
        // may not.
        let on_the_cloud = turn.brain.unwrap_or(asker.brain).cloud();
        let found = self.store.search_record(&query)?;
        let chosen = record::choose(found.clone(), &terms, on_the_cloud);
        if chosen.iter().any(|e| {
            found.iter().any(|p| {
                p.locator == e.locator && !crate::domain::history::may_reach_a_cloud_member(p.tier)
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
        if task.is_none() && self.running(run)?.task.is_none() {
            return self.consult_peer_in(run, expert, question, None, None, &[]);
        }
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
        let off = self.settings.offset_minutes;
        let at = self.clock.now();
        let today = money::day_of(at, off);
        // Identity/rank checks apply even to a retry. Admission and shared quota checks
        // then commit together, with replay checked before consuming another allowance.
        consult::check(&asker, &expert, 0, 0).map_err(refused)?;
        let intent = Consult {
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
            at,
        };
        let (mut asked, fresh) = self
            .store
            .accept_consult(&intent, today, money::day_start(today, off))?
            .map_err(refused)?;
        if !fresh && asked.status != Status::Queued {
            let outcome = if asked.status == Status::FromRecord {
                "answered from the record"
            } else {
                "answered"
            };
            return Ok(format!(
                "This consultation has already been {outcome} in {}'s thread; read the saved answer.",
                Ref::Task(thread).said()
            ));
        }
        if !fresh && asked.answer_run.is_some() {
            return Ok(format!(
                "Already asked {}. Their answer returns in {}'s thread; the existing expert turn is retained.",
                expert.name,
                Ref::Task(thread).said()
            ));
        }
        if let Some(why) = consult::cannot_be_woken(&expert) {
            self.answer_from_record(&mut asked, &format!("{why}, so it is not woken"))?;
            return Ok(format!(
                "{} {why} and was not woken. The record answered in {}'s thread, from what they \
                 said: you are woken to read it.",
                expert.name,
                Ref::Task(thread).said()
            ));
        }
        if self.deliver_consult(&asked, ConsultDelivery::Expert)? != Receipt::Queued {
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

    /// Compatibility entry point for a taskless ordinary consult. The owner-created scope
    /// supplies participants and payer; this never creates a conversation or task.
    /// # Errors
    /// The same live-turn, scope, source and execution limits as explicit assistance.
    pub fn consult_peer_in(
        &self,
        run: RunId,
        expert: &str,
        question: &str,
        conversation: Option<i64>,
        key: Option<&str>,
        evidence: &[crate::domain::assistance::SourceRef],
    ) -> Done<String> {
        let turn = self.running(run)?;
        if turn.task.is_some() {
            return Err(EngineError::Refused(Refusal::Conversation(
                "task-owned consultation uses its task thread",
            )));
        }
        let expert = self.named(expert.trim())?;
        if !self.conversations_enabled()? {
            return Err(refused(HistoryRefusal::NeedsTask));
        }
        let scope = match conversation {
            Some(id) => {
                self.store
                    .conversation(id)?
                    .ok_or(EngineError::Refused(Refusal::Conversation(
                        "conversation unavailable",
                    )))?
            }
            None => {
                let scopes = self.store.conversation_pair(turn.member, expert.id)?;
                if scopes.len() != 1 {
                    return Err(EngineError::Refused(Refusal::Conversation(
                        "select an owner-authorized conversation with read_messages; no unique conversation exists for this pair",
                    )));
                }
                scopes.into_iter().next().expect("one scope")
            }
        };
        if !scope.participants.contains(&turn.member)
            || !scope.participants.contains(&expert.id)
            || turn.member == expert.id
        {
            return Err(EngineError::Refused(Refusal::Conversation(
                "the conversation does not authorize this colleague pair",
            )));
        }
        // Fixed FNV-1a-128 supplies a stable retry label, not an authorization boundary.
        // The adapter also compares the full payload and refuses a colliding/different one.
        let hash = question.as_bytes().iter().fold(
            0x6c62_272e_07bb_0142_62b8_2175_6295_c58d_u128,
            |hash, byte| {
                (hash ^ u128::from(*byte)).wrapping_mul(0x0000_0000_0100_0000_0000_0000_0000_013b)
            },
        );
        let retry = format!("consult_{}_{hash:032x}", run.0);
        let id =
            self.request_assistance_in(run, scope.id, key.unwrap_or(&retry), question, evidence)?;
        let request = self
            .store
            .assistance_request(id)?
            .ok_or(EngineError::Unknown("assistance request"))?;
        Ok(serde_json::json!({"request":id,"conversation":scope.id,"state":request.status,
            "scheduled":matches!(request.status.as_str(),"queued"|"processing"),
            "reply":"The assigned expert answers with answer_assistance; the result returns in this conversation. End your turn when finished."}).to_string())
    }

    /// The consult a turn is the answer to, if it is one.
    pub(super) fn consult_of_run(&self, run: RunId) -> Done<Option<Consult>> {
        Ok(self.store.consult_for_run(run)?)
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
    pub(super) fn claim_consult(&self, run: &Run) -> Done<Option<Consult>> {
        Ok(self.store.claim_consult(run.id, run.member)?)
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
        found.status = Status::Answered;
        self.finish_consult_answer(&found, Author::Member(member.id), &text)?;
        self.wake_the_asker(&mut found)
    }

    /// The record answers a consult the member could not, or did not: what they said about it
    /// in the last ninety days, as testimony, posted in the asker's thread by HQ.
    fn answer_from_record(&self, found: &mut Consult, why: &str) -> Done<()> {
        let expert = self.member(found.expert)?;
        let terms = record::terms(&found.question);
        let query = Query {
            terms: terms.clone(),
            since: record::since(self.clock.now()),
            member: Some(expert.name.clone()),
            task: None,
            project: None,
        };
        // The asker reads the thread in a later turn that may judge on Claude (ADR 0131), so
        // nothing a turn read in Confidence is posted into it.
        let chosen = if terms.is_empty() {
            Vec::new()
        } else {
            record::choose(self.store.search_record(&query)?, &terms, true)
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
        found.status = Status::FromRecord;
        found.why = why.to_owned();
        self.finish_consult_answer(found, Author::Endora, text.trim_end())?;
        self.wake_the_asker(found)
    }

    fn finish_consult_answer(&self, found: &Consult, author: Author, text: &str) -> Done<()> {
        self.task(found.thread)?;
        let post = crate::application::ports::Post {
            author: self.author_name(author)?,
            text: text.to_owned(),
            at: self.clock.now(),
            unsupported: false,
        };
        // record::clean neutralizes the same word boundaries that mentions() recognizes;
        // peer testimony cannot create mention wakes. Resolve knowledge touches before
        // committing so a failed effect leaves the entire answer retryable.
        let mut effects = ConsultPostEffects::default();
        if let Author::Member(member) = author {
            effects.member = Some(member);
            effects.stories = self
                .store
                .stories()?
                .into_iter()
                .filter(|story| story.cited_in(text))
                .map(|story| story.id)
                .collect();
            let lower = text.to_ascii_lowercase();
            effects.sections = self
                .store
                .sections()?
                .into_iter()
                .filter(|section| lower.contains(&section.address()))
                .map(|section| section.id)
                .collect();
        }
        if !self
            .store
            .finish_consult_with_effects(found, &post, &effects)?
        {
            return Err(refused(HistoryRefusal::NoConsultForYou));
        }
        Ok(())
    }

    /// Wakes the asker to read the answer. If its turn is still running the wake would only
    /// join it; the asker is woken when that turn ends.
    fn wake_the_asker(&self, found: &mut Consult) -> Done<()> {
        let receipt = self.deliver_consult(found, ConsultDelivery::Answer)?;
        found.asker_woken = receipt != Receipt::Folded;
        Ok(())
    }

    fn deliver_consult(&self, found: &Consult, stage: ConsultDelivery) -> Done<Receipt> {
        let (member, task, reason) = match stage {
            ConsultDelivery::Expert => (found.expert, None, WakeReason::Consulted),
            ConsultDelivery::Answer => (
                found.asker,
                Some(found.thread),
                WakeReason::QuestionAnswered,
            ),
        };
        let (decision, coordination_held) = self.decide_wake(member, task, reason)?;
        let receipt = self.store.deliver_consult(
            found.id,
            stage,
            decision,
            self.asked_by_owner(),
            self.clock.now(),
        )?;
        if let (true, Some(task)) = (receipt == Receipt::Held && coordination_held, task) {
            self.coordination_held_notice(task)?;
        }
        if let (Receipt::Refused(crate::domain::Refused::Redundant), Some(task)) = (receipt, task) {
            self.found_redundant(member, task)?;
        }
        Ok(receipt)
    }

    /// Recover bounded pending intents through the existing policy and queue. Legacy
    /// task-thread consultation remains available independently of the conversation switch.
    pub(super) fn consult_delivery_tick(&self) -> Done<()> {
        if self.draining() {
            return Ok(());
        }
        for mut found in self.store.pending_consult_dispatches()? {
            let expert = self.member(found.expert)?;
            if let Some(why) = consult::cannot_be_woken(&expert) {
                self.answer_from_record(&mut found, &format!("{why}, so it is not woken"))?;
                continue;
            }
            if self.deliver_consult(&found, ConsultDelivery::Expert)? != Receipt::Queued {
                self.answer_from_record(&mut found, "could not be woken just now")?;
            }
        }
        for mut found in self.store.ended_consults()? {
            self.answer_from_record(&mut found, "did not answer")?;
        }
        for mut found in self.store.pending_consult_notices()? {
            self.wake_the_asker(&mut found)?;
        }
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
        let unread = self.store.unread_consults(run.member)?;
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
}

/// `text` cut to `most` characters, with an ellipsis if it was.
fn cut(text: &str, most: usize) -> String {
    if text.chars().count() <= most {
        return text.to_owned();
    }
    let kept: String = text.chars().take(most.saturating_sub(1)).collect();
    format!("{}…", kept.trim_end())
}
