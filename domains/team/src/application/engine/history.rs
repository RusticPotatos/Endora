//! The team's history (ADR 0129): the conversation of every turn, kept for a while. The node
//! sends a turn's conversation (scrubbed of secrets, by the same scrubber as its receipts) with
//! the turn's token while the turn runs; HQ caps it, marks the tier of what the turn read, and
//! keeps it. A tick lets go of what is past thirty days and of all but the last five of a member
//! on a task. Handover notes and posts (a turn's last words among them) are not touched.

use super::{Done, Engine, EngineError};
use crate::application::ports::{Clock, TeamStore, WorkerAdapter};
use crate::domain::history::{self, HistoryRefusal, Transcript};
use crate::domain::situations;
use crate::domain::{Millis, Refusal, RunId, Runtime};

/// How often the retention rule is looked at: an hour.
const PRUNE_EVERY_MS: Millis = 60 * 60 * 1000;

/// The setting that keeps when the retention rule last ran.
const PRUNED_AT: &str = "transcripts_pruned_at";

/// What keeping a conversation came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TranscriptKept {
    /// About how many tokens it comes to.
    pub est_tokens: i64,
    /// Whether the middle was cut to fit.
    pub cut: bool,
    /// The tier of what it read: Confidential if it read Confidential data, else Internal.
    pub tier: endora_kernel::tier::Tier,
}

impl<S: TeamStore, W: WorkerAdapter, C: Clock> Engine<S, W, C> {
    /// Before publishing the protocol at startup, close legacy peer work while
    /// retaining its history. The retired protocol must admit no new work.
    /// # Errors
    /// A store failure; startup must fail rather than serve a partial cutover.
    pub fn retire_legacy_peer_work(&self) -> Done<usize> {
        let _held = self.held();
        let mut stopped = 0;
        loop {
            let batch = self.store.retire_legacy_consultations()?;
            if batch.cancelled == 0 && batch.runs.is_empty() {
                return Ok(stopped);
            }
            for id in batch.runs {
                let Some(run) = self.store.run(id)?.filter(|run| run.status.live()) else {
                    continue;
                };
                if run.status == crate::domain::RunStatus::Running {
                    self.runner.stop(id, std::time::Duration::from_secs(5));
                }
                self.end(
                    run,
                    crate::domain::RunStatus::Stopped,
                    "legacy peer help retired at protocol v2 cutover",
                )?;
                stopped += 1;
            }
        }
    }

    /// Owner inspection of retained task questions, independent of current peer permissions.
    /// # Errors
    /// An unknown task or a store failure.
    pub fn legacy_task_conversations(
        &self,
        task: crate::domain::TaskId,
        after: i64,
    ) -> Done<serde_json::Value> {
        let _held = self.held();
        self.task(task)?;
        let after = after.max(0);
        let mut rows = self.store.legacy_task_consultations(task, after)?;
        let more = rows.len() > 50;
        rows.truncate(50);
        let cursor = rows.last().map_or(after, |row| row.id);
        let records: Vec<_> = rows.into_iter().map(|row| {
            let answer = row.answer.map(|answer| {
                let origin = match answer.origin {
                    history::InputOrigin::Owner => serde_json::json!({"kind":"owner"}),
                    history::InputOrigin::Run(run) => serde_json::json!({"kind":"run","run":run.0}),
                    history::InputOrigin::Unknown => serde_json::json!({"kind":"unknown"}),
                    history::InputOrigin::System => serde_json::json!({"kind":"system"}),
                };
                serde_json::json!({"id":answer.id,"author":answer.post.author,"text":answer.post.text,
                    "at":answer.post.at,"unsupported":answer.post.unsupported,"origin":origin,
                    "evidence":answer.evidence.iter().map(|e|e.said()).collect::<Vec<_>>()})
            });
            serde_json::json!({"id":format!("legacy-consult:{}",row.id),"legacy_id":row.id,
                "requester":row.requester.0,"requester_run":row.requester_run.0,
                "expert":row.expert.0,"expert_run":row.expert_run.map(|id|id.0),
                "thread":row.thread.0,"topic":row.topic.map(|id|id.0),"project":row.project.map(|id|id.0),
                "question":row.question,"outcome":row.outcome,"reason":row.reason,"at":row.at,
                "answer_post":row.answer_post,"answer":answer})
        }).collect();
        Ok(
            serde_json::json!({"source":"legacy-task-consultation","task":task.0,
            "records":records,"after":cursor,"has_more":more}),
        )
    }

    /// The node tells HQ a turn's conversation, as plain text already scrubbed of secrets,
    /// naming the turn by its own token: kept only while that turn runs, so neither a model nor
    /// a turn that has ended can write one. Capped at [`history::TRANSCRIPT_MOST`] bytes (head
    /// and tail, the cut marked), marked with the tier of what the turn read, and kept until
    /// thirty days from now. Nothing is kept for text that is empty.
    ///
    /// # Errors
    /// [`EngineError::Unknown`] for a token that opens no running turn;
    /// [`HistoryRefusal::TranscriptKept`] for a turn that has one; a store failure.
    pub fn record_transcript(
        &self,
        token: &str,
        runtime: &str,
        text: &str,
    ) -> Done<Option<TranscriptKept>> {
        let _held = self.held();
        let Some(id) = self.store.run_of_token(token)? else {
            return Err(EngineError::Unknown("running turn with that token"));
        };
        let run = self
            .running(id)
            .map_err(|_| EngineError::Unknown("running turn with that token"))?;
        if text.trim().is_empty() {
            return Ok(None);
        }
        if self.store.transcript(run.id)?.is_some() {
            return Err(EngineError::Refused(Refusal::History(
                HistoryRefusal::TranscriptKept,
            )));
        }
        let member = self.member(run.member)?;
        let confidential = self.store.run_read_confidential(run.id)?;
        let (body, cut) = history::cap(text, history::TRANSCRIPT_MOST);
        let now = self.clock.now();
        let kept = Transcript {
            run: run.id,
            member: member.id,
            task: run.task,
            runtime: Runtime::from_name(runtime)
                .unwrap_or_default()
                .name()
                .to_owned(),
            // The brain the turn ran on (ADR 0131), not the one its member judges with.
            brain: run.brain.unwrap_or(member.brain).name().to_owned(),
            tier: history::tier_of(confidential),
            est_tokens: i64::try_from(situations::tokens(&body)).unwrap_or(i64::MAX),
            cut,
            body,
            at: now,
            kept_until: now + history::KEPT_DAYS * history::DAY_MS,
        };
        self.store.add_transcript(&kept)?;
        Ok(Some(TranscriptKept {
            est_tokens: kept.est_tokens,
            cut,
            tier: kept.tier,
        }))
    }

    /// A turn's conversation, if HQ kept it and still has it.
    ///
    /// # Errors
    /// A store failure.
    pub fn transcript_of(&self, run: RunId) -> Done<Option<Transcript>> {
        let _held = self.held();
        Ok(self.store.transcript(run)?)
    }

    /// Lets go of the conversations past their thirty days, and of all but the last five of each
    /// member on each task, now. Returns how many went. The tick does this about hourly.
    ///
    /// # Errors
    /// A store failure.
    pub fn prune_transcripts(&self) -> Done<usize> {
        let _held = self.held();
        self.prune_held()
    }

    fn prune_held(&self) -> Done<usize> {
        let gone = history::to_prune(&self.store.transcripts_kept()?, self.clock.now());
        if gone.is_empty() {
            return Ok(0);
        }
        Ok(self.store.forget_transcripts(&gone)?)
    }

    /// The tick's look at the retention rule (the lock held): about once an hour.
    pub(super) fn history_tick(&self, now: Millis) -> Done<()> {
        let last: Millis = self
            .store
            .setting(PRUNED_AT)?
            .and_then(|at| at.parse().ok())
            .unwrap_or(Millis::MIN);
        if now.saturating_sub(last) < PRUNE_EVERY_MS {
            return Ok(());
        }
        self.store.set_setting(PRUNED_AT, &now.to_string())?;
        self.prune_held()?;
        Ok(())
    }
}
