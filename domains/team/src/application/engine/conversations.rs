//! Information-only conversations: no task, wake, model call or capability grant.

use serde_json::{Value, json};

use super::{Done, Engine, EngineError};
use crate::application::ports::{Clock, TeamStore, WorkerAdapter};
use crate::domain::conversation::{self, Conversation, Information};
use crate::domain::notebook::Note;
use crate::domain::{MemberId, ProjectId, ProjectStatus, Ref, Refusal, Role, RunId};

fn refused(reason: &'static str) -> EngineError {
    EngineError::Refused(Refusal::Conversation(reason))
}

fn shown(c: &Conversation) -> Value {
    json!({"id":c.id,"project":c.project.0,"participants":[c.participants[0].0,c.participants[1].0],
        "at":c.at,"latest":c.latest,"messages":c.messages})
}

impl<S: TeamStore, W: WorkerAdapter, C: Clock> Engine<S, W, C> {
    /// Discovery uses trusted task metadata and an indexed scope existence check,
    /// not the first presentation page. Admission still revalidates authority.
    pub(crate) fn has_communication_scope(
        &self,
        run: &crate::application::ports::Run,
    ) -> Done<bool> {
        let project = match run.task {
            Some(task) => match self.task(task)?.project {
                Some(project) => Some(project),
                None => return Ok(false),
            },
            None => None,
        };
        // One team (ADR 0139): a task of an active project may ask any colleague, the pair's
        // conversation made when first asked.
        if let Some(project) = project {
            if self
                .project(project)
                .is_ok_and(|p| p.status == ProjectStatus::Active)
            {
                return Ok(true);
            }
        }
        Ok(self.store.has_active_conversation(run.member, project)?)
    }

    /// Whether this turn is offered the colleague tools (`consult`, `request_assistance`,
    /// `send_message`): the one rule both the tool list and the briefing follow, read after
    /// the briefing has recorded what it showed the turn.
    /// # Errors
    /// A store failure.
    pub(crate) fn may_ask_colleagues(
        &self,
        member: &crate::domain::Member,
        run: &crate::application::ports::Run,
    ) -> Done<bool> {
        // A Confidential turn may ask too: its colleague answers on the house's models
        // (ADR 0140), which every turn of work is on.
        Ok(member.role != crate::domain::Role::Ceo
            && self.conversation_turn_ready(run.id)?
            && self.has_communication_scope(run)?)
    }

    fn conversation_revision(&self) -> Done<Option<i64>> {
        let stored = self.store.setting("conversations_enabled")?;
        Ok(stored
            .as_deref()
            .and_then(|s| s.strip_prefix("1:"))
            .and_then(|n| n.parse::<i64>().ok()))
    }

    /// A turn only uses this new tool if its own briefing was classified while the
    /// feature was enabled at the same owner switch revision.
    pub fn conversation_turn_ready(&self, run: RunId) -> Done<bool> {
        let Some(revision) = self.conversation_revision()? else {
            return Ok(false);
        };
        Ok(self.store.conversation_ready(run, revision)?)
    }

    /// Called while composing the briefing, before the runner receives it.
    pub(super) fn prepare_conversation_turn(&self, run: RunId) -> Done<()> {
        if let Some(revision) = self.conversation_revision()? {
            self.store.mark_conversation_ready(run, revision)?;
        }
        Ok(())
    }

    /// Carry a source turn's observed classification to this one. A missing source is
    /// unclassified legacy material and fails closed for Internal messaging.
    pub(super) fn inherit_input_from(&self, run: RunId, source: Option<RunId>) -> Done<()> {
        if !self.conversations_enabled()? {
            return Ok(());
        }
        let confidential = match source {
            Some(id) => self.store.run_read_confidential(id)?,
            _ => true,
        };
        if confidential {
            self.store.mark_confidential_input(run)?;
        }
        Ok(())
    }

    /// A returned notebook contains every listed note, including model-written history.
    pub(super) fn inherit_notebook_inputs(&self, run: RunId, notes: &[Note]) -> Done<()> {
        for note in notes {
            if !note.from_owner {
                self.inherit_input_from(run, note.written_in_run)?;
            }
        }
        Ok(())
    }

    /// Legacy text without proven source lineage cannot be relabelled Internal by a new tool.
    pub(crate) fn unclassified_message_input(&self, run: RunId) -> Done<()> {
        if !self.conversations_enabled()? {
            return Ok(());
        }
        self.store.mark_confidential_input(run)?;
        Ok(())
    }

    /// Read only the requested tail and record its source tiers before returning text.
    pub(crate) fn latest_task_posts(
        &self,
        run: RunId,
        task: crate::domain::TaskId,
        most: usize,
    ) -> Done<Vec<crate::application::history_ports::PostInput>> {
        let posts = self.store.latest_authored_posts(task, most)?;
        for post in &posts {
            self.inherit_authored_input(run, post.origin)?;
        }
        Ok(posts)
    }

    pub(super) fn inherit_authored_input(
        &self,
        run: RunId,
        origin: crate::domain::history::InputOrigin,
    ) -> Done<()> {
        match origin {
            // HQ's own words borrow nothing, so like the owner's they carry no tier.
            crate::domain::history::InputOrigin::Owner
            | crate::domain::history::InputOrigin::System => Ok(()),
            crate::domain::history::InputOrigin::Run(source) => {
                self.inherit_input_from(run, Some(source))
            }
            crate::domain::history::InputOrigin::Unknown => self.unclassified_message_input(run),
        }
    }

    /// Whether the owner enabled information-only messaging; absent is off.
    /// # Errors
    /// A store failure.
    pub fn conversations_enabled(&self) -> Done<bool> {
        Ok(self.conversation_revision()?.is_some())
    }

    /// The owner's switch. No worker tool exposes this operation.
    /// # Errors
    /// A store failure.
    pub fn set_conversations_enabled(&self, enabled: bool) -> Done<()> {
        let _held = self.held();
        let prior = self
            .store
            .setting("conversations_enabled")?
            .unwrap_or_default();
        let revision = prior
            .split_once(':')
            .and_then(|(_, n)| n.parse::<i64>().ok())
            .unwrap_or(0)
            .saturating_add(1);
        self.store.set_setting(
            "conversations_enabled",
            &format!("{}:{revision}", if enabled { 1 } else { 0 }),
        )?;
        if !enabled {
            self.store.cancel_assistance()?;
            self.stop_inactive_assistance_turns()?;
        }
        Ok(())
    }

    /// The owner explicitly authorizes two fixed non-CEO participants in an active project.
    /// # Errors
    /// Unknown records, inactive project, disabled pilot or ineligible participants.
    pub fn create_conversation(
        &self,
        project: ProjectId,
        participants: [MemberId; 2],
    ) -> Done<i64> {
        let _held = self.held();
        if !self.conversations_enabled()? {
            return Err(refused("information messaging is disabled"));
        }
        if self.project(project)?.status != ProjectStatus::Active
            || participants[0] == participants[1]
        {
            return Err(refused(
                "a conversation needs an active project and two distinct members",
            ));
        }
        for id in participants {
            let member = self.member(id)?;
            if member.role == Role::Ceo || !member.at_work() {
                return Err(refused("the pilot needs two active non-CEO members"));
            }
        }
        Ok(self
            .store
            .add_conversation(project, participants, self.clock.now())?)
    }

    pub(super) fn conversation_for(
        &self,
        run: RunId,
        conversation: i64,
        operation: &'static str,
    ) -> Done<Conversation> {
        let turn = self.running(run)?;
        let member = self.member(turn.member)?;
        let scope = self.store.conversation(conversation)?;
        let allowed = scope
            .as_ref()
            .is_some_and(|s| s.participants.contains(&member.id));
        let reason = if !self.conversations_enabled()? {
            Some("information messaging is disabled")
        } else if !allowed || !member.at_work() || member.role == Role::Ceo {
            Some("conversation unavailable")
        } else if !self.conversation_turn_ready(run)? {
            Some("this turn began before the current messaging policy")
        } else {
            None
        };
        if let Some(reason) = reason {
            self.store
                .conversation_operation(run, None, operation, reason, self.clock.now())?;
            return Err(refused(reason));
        }
        let scope = scope.ok_or_else(|| refused("conversation unavailable"))?;
        if self.project(scope.project)?.status != ProjectStatus::Active {
            self.store.conversation_operation(
                run,
                Some(conversation),
                operation,
                "project inactive",
                self.clock.now(),
            )?;
            return Err(refused("the conversation's project is not active"));
        }
        Ok(scope)
    }

    /// A bounded directory for worker discovery or owner inspection, even when disabled.
    /// # Errors
    /// A store failure.
    pub fn conversation_directory(&self, member: Option<MemberId>, after: i64) -> Done<Value> {
        let mut rows = self.store.conversation_page(member, after.max(0))?;
        let more = rows.len() > conversation::PAGE;
        rows.truncate(conversation::PAGE);
        Ok(
            json!({"enabled":self.conversations_enabled()?,"conversations":rows.iter().map(shown).collect::<Vec<_>>(),
            "has_more":more,"after":rows.last().map_or(after,|c|c.id)}),
        )
    }

    /// Owner's bounded operation history, separate from message contents and the general feed.
    /// # Errors
    /// A store failure.
    pub fn conversation_operations(&self, after: i64) -> Done<Value> {
        let mut rows = self.store.conversation_operations_page(after.max(0))?;
        let more = rows.len() > conversation::PAGE;
        rows.truncate(conversation::PAGE);
        Ok(
            json!({"operations":rows.iter().map(|o| json!({"id":o.id,"conversation":o.conversation,
            "member":o.member.0,"run":o.run.0,"operation":o.operation,"reason":o.reason,"at":o.at})).collect::<Vec<_>>(),
            "has_more":more,"after":rows.last().map_or(after,|o|o.id)}),
        )
    }

    /// Attribute a protocol refusal without keeping untrusted text or a claimed scope.
    /// # Errors
    /// An ended turn or a store failure.
    pub(crate) fn conversation_protocol_refused(&self, run: RunId, tool: &str) -> Done<()> {
        let _held = self.held();
        self.running(run)?;
        self.store
            .conversation_operation(run, None, tool, "protocol refused", self.clock.now())?;
        Ok(())
    }

    /// Worker discovery rechecks current eligibility and shows only active project scopes.
    /// # Errors
    /// An ended/ineligible turn, disabled pilot or store failure.
    pub fn conversation_directory_in(&self, run: RunId, after: i64) -> Done<Value> {
        let _held = self.held();
        let turn = self.running(run)?;
        let member = self.member(turn.member)?;
        let allowed = self.conversations_enabled()?
            && member.at_work()
            && member.role != Role::Ceo
            && self.conversation_turn_ready(run)?;
        self.store.conversation_operation(
            run,
            None,
            "directory",
            if allowed { "accepted" } else { "refused" },
            self.clock.now(),
        )?;
        if !allowed {
            return Err(refused(
                "information messaging is unavailable for this turn",
            ));
        }
        let mut page = self.conversation_directory(Some(member.id), after)?;
        if let Some(rows) = page["conversations"].as_array_mut() {
            let mut active = Vec::new();
            for row in rows.drain(..) {
                let project = ProjectId(
                    row["project"]
                        .as_i64()
                        .ok_or_else(|| refused("project unavailable"))?,
                );
                if self.project(project)?.status == ProjectStatus::Active {
                    active.push(row);
                }
            }
            *rows = active;
        }
        Ok(page)
    }

    /// Accept a canonical information record without scheduling anything.
    /// # Errors
    /// An ended/unauthorized turn, input bounds, confidential context, key conflict or quota.
    pub fn send_message_in(
        &self,
        run: RunId,
        conversation: i64,
        key: &str,
        text: &str,
        evidence: &[Ref],
    ) -> Done<i64> {
        let _held = self.held();
        self.conversation_for(run, conversation, "send")?;
        let turn = self.running(run)?;
        let result = (|| {
            let text = conversation::words(key, text, evidence).map_err(refused)?;
            self.all_there(evidence)?;
            self.store
                .accept_information(&Information {
                    conversation,
                    member: turn.member,
                    run,
                    key: key.to_owned(),
                    text,
                    evidence: evidence.to_vec(),
                    day: self.today(),
                    at: self.clock.now(),
                })?
                .map_err(refused)
        })();
        if result.is_err() {
            let reason = match &result {
                Err(EngineError::Refused(Refusal::Conversation(reason))) => *reason,
                _ => "evidence or storage refused",
            };
            self.store.conversation_operation(
                run,
                Some(conversation),
                "send",
                reason,
                self.clock.now(),
            )?;
        }
        result
    }

    /// A participant reads a bounded page. Information reads never wake or claim work.
    /// # Errors
    /// An ended or unauthorized turn, disabled pilot or store failure.
    pub fn read_messages_in(&self, run: RunId, conversation: i64, after: i64) -> Done<Value> {
        let _held = self.held();
        self.conversation_for(run, conversation, "read")?;
        let mut page = self.conversation_messages(conversation, after)?;
        let mut sources = Vec::new();
        for (source, _) in self
            .store
            .assistance_grants(conversation, self.clock.now())?
        {
            if self.assistance_source_allowed(conversation, source)? {
                sources.push(source.said());
            }
        }
        page["assistance_sources"] = json!(sources);
        self.store.conversation_operation(
            run,
            Some(conversation),
            "read",
            "accepted",
            self.clock.now(),
        )?;
        Ok(page)
    }

    /// Owner's inspection/export page; available when the feature is disabled.
    /// # Errors
    /// A deleted/missing conversation or store failure.
    pub fn conversation_messages(&self, id: i64, after: i64) -> Done<Value> {
        let scope = self
            .store
            .conversation(id)?
            .ok_or_else(|| refused("conversation unavailable"))?;
        let mut messages = self.store.message_page(id, after.max(0))?;
        let more = messages.len() > conversation::PAGE;
        messages.truncate(conversation::PAGE);
        Ok(json!({"conversation":shown(&scope),"has_more":more,
            "after":messages.last().map_or(after,|m|m.id),
            "messages":messages.iter().map(|m|json!({"id":m.id,"conversation":m.conversation,
                "member":m.member.0,"run":m.run.0,"text":m.text,"at":m.at,
                "evidence":m.evidence.iter().map(|r|r.said()).collect::<Vec<_>>(),
                "kind":"information","tier":"internal"})).collect::<Vec<_>>()}))
    }

    /// Owner deletion revokes reads immediately; the tick purges bounded batches.
    /// # Errors
    /// A store failure or unknown conversation.
    pub fn delete_conversation(&self, id: i64) -> Done<()> {
        let _held = self.held();
        if !self.store.delete_conversation(id, self.clock.now())? {
            return Err(refused("conversation unavailable"));
        }
        self.stop_inactive_assistance_turns()?;
        Ok(())
    }
}
