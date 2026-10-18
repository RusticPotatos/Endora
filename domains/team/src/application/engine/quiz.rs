//! The nightly quiz on the map (ADR 0126). At night, if a member on the house's model is free, HQ
//! wakes it for a downtime turn that carries a few questions code made from that member's own
//! map; the member answers with `answer_quiz` and code scores the answers against the same
//! facts. No paid turn is ever spent on it: when no such member is free, the night is recorded
//! as skipped. A page that fails night after night is flagged to its steward and the World card.

use serde_json::{Value, json};

use super::office::downtime_day;
use super::{Done, Engine, EngineError};
use crate::application::ports::{Clock, TeamStore, WorkerAdapter};
use crate::domain::map::{self, PageKey, QUIZ_OPEN_MS};
use crate::domain::quiz::{self, Quiz, Status};
use crate::domain::world::{DAY_MS, WorldRefusal};
use crate::domain::{EventKind, Member, Millis, Receipt, Refusal, Role, RunId, WakeReason};

/// The quiz is asked from this hour of the owner's night...
const FROM_HOUR: i64 = 1;
/// ...and not after this one: past it the night is recorded as skipped.
const UNTIL_HOUR: i64 = 6;
/// An hour, in milliseconds.
const HOUR_MS: Millis = 60 * 60 * 1000;

fn refused(r: WorldRefusal) -> EngineError {
    EngineError::Refused(Refusal::World(r))
}

impl<S: TeamStore, W: WorkerAdapter, C: Clock> Engine<S, W, C> {
    /// The look at the night's quiz (the lock held): quizzes asked and not answered in twelve
    /// hours are let go; and, once a night between one and six in the owner's time, one free
    /// member on the house's model is woken for a downtime turn with a quiz on its map. Past six
    /// with none asked, the night is recorded as skipped, with why.
    pub(super) fn quiz_look(&self, now: Millis) -> Done<()> {
        let off = self.settings.offset_minutes;
        let night = map::night_of(now, off);
        let hour = (now + off * 60_000).rem_euclid(DAY_MS) / HOUR_MS;
        let mut kept = self.store.quizzes()?;
        for q in &mut kept {
            let late = q.asked_at.is_some_and(|at| now - at > QUIZ_OPEN_MS);
            if q.status == Status::Asked && late {
                q.status = Status::Unanswered;
                q.why = "not answered within twelve hours".to_owned();
                self.store.save_quiz(q)?;
                self.emit(
                    EventKind::WorldChanged,
                    json!({ "what": "quiz unanswered", "night": q.night, "by": "Endora" }),
                )?;
            }
        }
        if kept.iter().any(|q| q.night == night) || hour < FROM_HOUR {
            return Ok(());
        }
        if hour >= UNTIL_HOUR {
            return self.quiz_skipped(
                night,
                "the night passed with no member on the house's model free",
            );
        }
        let Some(member) = self.quiz_candidate()? else {
            return Ok(());
        };
        let questions = quiz::questions(&self.map_facts(&member, None)?, night);
        if questions.len() < quiz::LEAST_QUESTIONS {
            return self.quiz_skipped(night, "its map was too short to ask about");
        }
        let mut asked = Quiz {
            id: 0,
            night,
            status: Status::Asked,
            member: Some(member.id),
            asked_at: Some(now),
            answered_at: None,
            questions,
            answers: Vec::new(),
            graded: Vec::new(),
            why: String::new(),
        };
        asked.id = self.store.save_quiz(&asked)?;
        // It takes the member's downtime for the day, so none follows it.
        self.store
            .set_setting(&downtime_day(member.id), &self.today().to_string())?;
        if self.wake_held_lock(member.id, None, WakeReason::Downtime)? != Receipt::Queued {
            asked.status = Status::Skipped;
            asked.why = "its wake was turned away".to_owned();
            self.store.save_quiz(&asked)?;
            return Ok(());
        }
        self.emit(
            EventKind::WorldChanged,
            json!({ "what": "quiz asked", "night": night, "member": member.name,
                    "questions": asked.questions.len(), "by": "Endora" }),
        )
    }

    fn quiz_skipped(&self, night: i64, why: &str) -> Done<()> {
        self.store.save_quiz(&Quiz {
            id: 0,
            night,
            status: Status::Skipped,
            member: None,
            asked_at: None,
            answered_at: None,
            questions: Vec::new(),
            answers: Vec::new(),
            graded: Vec::new(),
            why: why.to_owned(),
        })?;
        self.emit(
            EventKind::WorldChanged,
            json!({ "what": "quiz skipped", "night": night, "why": why, "by": "Endora" }),
        )
    }

    /// The member to ask: on the house's model (so it costs nothing), at work, not the CEO,
    /// with no turn live, no wake held and no open work; the one whose last downtime was the
    /// longest ago. None while any downtime turn is live: one at a time.
    fn quiz_candidate(&self) -> Done<Option<Member>> {
        let seen = self.downtimes()?;
        if seen.live {
            return Ok(None);
        }
        let runs = self.store.runs()?;
        let tasks = self.store.tasks()?;
        let wakes = self.store.wakes()?;
        let mut free: Vec<Member> = self
            .store
            .members()?
            .into_iter()
            .filter(|m| m.role != Role::Ceo && m.at_work() && !m.brain.cloud())
            .filter(|m| {
                let busy = runs.iter().any(|r| r.member == m.id && r.status.live())
                    || wakes
                        .iter()
                        .any(|w| w.member == m.id && w.receipt == Receipt::Held);
                let holds = tasks.iter().any(|t| {
                    !t.status.finished() && (t.owner == Some(m.id) || t.assignee == Some(m.id))
                });
                !busy && !holds
            })
            .collect();
        free.sort_by_key(|m| (seen.last.get(&m.id).copied().unwrap_or(Millis::MIN), m.id));
        Ok(free.into_iter().next())
    }

    /// A member's turn answers the quiz it was asked: one answer to each question, in order. Code
    /// scores them against the map and keeps the score. Returns how many were right and asked.
    ///
    /// # Errors
    /// [`Refusal::TurnOver`]; [`WorldRefusal::NoQuizForYou`] when no quiz is open for this member
    /// in this downtime turn; [`WorldRefusal::QuizAnswers`] for the wrong number of answers.
    pub fn answer_quiz_in(&self, run: RunId, answers: &[String]) -> Done<(usize, usize)> {
        let _held = self.held();
        let turn = self.running(run)?;
        let who = self.member(turn.member)?;
        let found = self
            .store
            .quizzes()?
            .into_iter()
            .rev()
            .find(|q| q.status == Status::Asked && q.member == Some(who.id));
        let Some(mut quiz) = found.filter(|_| self.in_downtime(run).unwrap_or(false)) else {
            return Err(refused(WorldRefusal::NoQuizForYou));
        };
        if answers.len() != quiz.questions.len() {
            return Err(refused(WorldRefusal::QuizAnswers(quiz.questions.len())));
        }
        quiz.answers = answers
            .iter()
            .map(|a| {
                a.chars()
                    .filter(|c| *c == ' ' || !c.is_control())
                    .take(200)
                    .collect()
            })
            .collect();
        quiz.graded = quiz::grade(&quiz.questions, &quiz.answers);
        quiz.status = Status::Answered;
        quiz.answered_at = Some(self.clock.now());
        self.store.save_quiz(&quiz)?;
        let (passed, asked) = (quiz.passed(), quiz.questions.len());
        self.emit(
            EventKind::WorldChanged,
            json!({ "what": "quiz answered", "night": quiz.night, "member": who.name,
                    "passed": passed, "asked": asked, "by": "Endora" }),
        )?;
        Ok((passed, asked))
    }

    /// What a downtime briefing says of tonight's quiz, if it was asked of `member`.
    pub(super) fn quiz_said(&self, member: &Member) -> Done<String> {
        let open = self
            .store
            .quizzes()?
            .into_iter()
            .rev()
            .find(|q| q.status == Status::Asked && q.member == Some(member.id));
        let Some(open) = open else {
            return Ok(String::new());
        };
        let mut said = String::from(
            "Tonight's quiz on your map: HQ made these questions from the map at the top of this \
             briefing and scores your answers by code; nothing rides on it but whether the map \
             carries. You may read_world. Then answer_quiz {answers: [...]}, one answer each, in \
             this order:\n",
        );
        for q in &open.questions {
            said.push_str(&format!("{}. {}\n", q.n + 1, q.text));
        }
        Ok(said)
    }

    /// The pages `member` keeps that failed the quiz three nights running, for its briefing;
    /// empty when none did.
    pub(super) fn quiz_flags_for(&self, member: &Member) -> Done<String> {
        let history = self.store.quizzes()?;
        let mut lines = Vec::new();
        for page in Self::quiz_pages(&history) {
            let Some(key) = PageKey::parse(&page) else {
                continue;
            };
            if quiz::flagged(&history, &page) && self.steward(key)?.is(member) {
                lines.push(format!("- world:{page} ({})\n", self.page_title(key)?));
            }
        }
        if lines.is_empty() {
            return Ok(String::new());
        }
        let mut said = String::from(
            "Pages of yours that failed the nightly quiz three nights running (a model with only \
             its map could not find or name them: make their title and first lines say what they \
             are):\n",
        );
        said.extend(lines);
        Ok(said)
    }

    /// Every page the quizzes have asked about, in the order first asked.
    fn quiz_pages(history: &[Quiz]) -> Vec<String> {
        let mut pages: Vec<String> = Vec::new();
        for g in history.iter().flat_map(|q| q.graded.iter()) {
            if !pages.contains(&g.page) {
                pages.push(g.page.clone());
            }
        }
        pages
    }

    /// The quiz as the World card shows it: last night's, and each page's score and flag.
    ///
    /// # Errors
    /// A store failure.
    pub fn quiz_shown(&self) -> Done<Value> {
        let history = self.store.quizzes()?;
        let off = self.settings.offset_minutes;
        let nights: Vec<Value> = history
            .iter()
            .rev()
            .take(quiz::SCORED_OVER)
            .map(|q| {
                let member = q
                    .member
                    .and_then(|m| self.member(m).ok())
                    .map(|m| m.name)
                    .unwrap_or_default();
                json!({ "night": q.night, "status": q.status.name(), "member": member,
                        "passed": q.passed(), "asked": q.graded.len().max(q.questions.len()),
                        "why": q.why, "at": q.asked_at,
                        "day": q.asked_at.map(|at| super::world::day_with_year(at, off)) })
            })
            .collect();
        let mut pages = Vec::new();
        for page in Self::quiz_pages(&history) {
            let (passed, asked) = quiz::score_of(&history, &page);
            let title = match PageKey::parse(&page) {
                Some(key) => self.page_title(key)?,
                None => page.clone(),
            };
            pages.push(
                json!({ "slug": page, "title": title, "passed": passed, "asked": asked,
                               "flagged": quiz::flagged(&history, &page) }),
            );
        }
        Ok(json!({ "enabled": self.settings.quiz, "nights": nights, "pages": pages }))
    }
}
