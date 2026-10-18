//! The open office (ADR 0119): the office as data, seeded once; proposals the team decides
//! together; the owner as landlord (undo, reset, veto); and downtime, a turn about no task
//! for a member with nothing to do. A model proposes and votes; the rules in
//! [`crate::domain::office`] and the count of votes decide, and a proposal is checked again
//! at the moment it would be made.

use std::collections::{HashMap, HashSet};

use serde_json::json;

use super::{Done, Engine, EngineError, one_line};
use crate::application::ports::{Clock, TeamStore, WorkerAdapter};
use crate::domain::office::{self, Change, Proposal, Status, Verdict, Vote};
use crate::domain::{
    EventKind, Member, MemberId, Micros, Millis, Refusal, Role, RunId, WakeId, WakeReason, money,
};

/// Set once HQ has put the default layout on the floor, so it never does again.
const OFFICE_SEEDED: &str = "office_seeded";
/// What the owner allows paid members' downtime turns together, in cents a day.
const DOWNTIME_PAID: &str = "downtime_paid_cents_per_day";
/// How long a member rests between downtime turns, so none follows another across midnight.
const DOWNTIME_REST_MS: Millis = 12 * 60 * 60 * 1000;
/// The most items a downtime briefing lists before it says how many more there are.
const BRIEFED_ITEMS: usize = 40;
/// The most closed proposals the snapshot remembers.
const RECENT_PROPOSALS: usize = 8;

/// Where a proposal stands after a vote or a look at the clock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Standing {
    /// Still waiting for votes.
    Open,
    /// It carried, and the office has changed.
    Applied,
    /// It was let go, and why.
    Dropped(String),
}

/// What the downtime turns so far say.
struct Downtimes {
    /// Whether a downtime turn is queued or running now.
    live: bool,
    /// When each member's last downtime turn ended (or began, while it runs).
    last: HashMap<MemberId, Millis>,
    /// What today's downtime turns hold or have spent, together.
    today: Micros,
}

fn proposal_said(by: &str, changes: &[String], why: &str) -> String {
    let mut text = format!("{by} proposes: {}", changes.join("; "));
    if !why.is_empty() {
        text.push_str(&format!(" \u{2014} '{why}'"));
    }
    text
}

impl<S: TeamStore, W: WorkerAdapter, C: Clock> Engine<S, W, C> {
    // ---- the office as data ---------------------------------------------------------------

    /// Puts the default layout on the floor, once: what the office has always had, as items
    /// the team may move. Returns whether it did. What the team removes later does not come
    /// back.
    ///
    /// # Errors
    /// A store failure.
    pub fn seed_office(&self) -> Done<bool> {
        let _held = self.held();
        if self.store.setting(OFFICE_SEEDED)?.is_some() {
            return Ok(false);
        }
        self.store.set_setting(OFFICE_SEEDED, "1")?;
        if !self.store.office_items()?.is_empty() {
            return Ok(false);
        }
        self.lay_out_the_default()?;
        Ok(true)
    }

    /// Replaces the floor with the default layout, new numbers and all.
    fn lay_out_the_default(&self) -> Done<()> {
        let fresh = office::apply(&[], &office::default_layout())
            .map_err(|r| EngineError::Refused(Refusal::Office(r)))?;
        self.store.replace_office_items(&[])?;
        let now = self.clock.now();
        for item in fresh {
            self.store
                .add_office_item(item.kind, item.x, item.y, "endora", now)?;
        }
        Ok(())
    }

    /// The things on the floor.
    ///
    /// # Errors
    /// A store failure.
    pub fn office_items(&self) -> Done<Vec<office::Item>> {
        Ok(self.store.office_items()?)
    }

    fn describe_all(&self, items: &[office::Item], changes: &[Change]) -> Vec<String> {
        changes.iter().map(|c| office::describe(items, c)).collect()
    }

    /// Marks the running turn as having changed something through HQ.
    fn moved_forward(&self, run: RunId) -> Done<()> {
        if let Some(mut found) = self.store.run(run)? {
            found.moved_forward = true;
            self.store.save_run(&found)?;
        }
        Ok(())
    }

    // ---- deciding together --------------------------------------------------------------

    /// A turn proposes changes to the office: one to five, each checked by the rules now and
    /// again when the proposal carries, and why. The proposer's own yes is counted. Returns
    /// the proposal's number.
    ///
    /// # Errors
    /// [`Refusal::TurnOver`]; [`Refusal::Office`] with what the rules refuse; a store failure.
    pub fn propose_office_in(&self, run: RunId, changes: &[Change], why: &str) -> Done<i64> {
        let _held = self.held();
        let turn = self.running(run)?;
        let who = self.member(turn.member)?;
        let refuse = |r| EngineError::Refused(Refusal::Office(r));
        office::check_count(changes).map_err(refuse)?;
        let items = self.store.office_items()?;
        office::apply(&items, changes).map_err(refuse)?;
        let now = self.clock.now();
        let why = one_line(why, office::WHY_MOST);
        let id = self
            .store
            .add_office_proposal(&who.name, changes, &why, now)?;
        self.store.add_office_vote(
            id,
            &Vote {
                who: who.name.clone(),
                yes: true,
                why: String::new(),
                at: now,
            },
        )?;
        let said = self.describe_all(&items, changes);
        self.emit(
            EventKind::OfficeProposed,
            json!({ "proposal": id, "by": who.name, "member": who.id.0, "why": why,
                    "text": proposal_said(&who.name, &said, &why) }),
        )?;
        self.moved_forward(run)?;
        Ok(id)
    }

    /// A turn votes on an open proposal, once; its own proposal already has its yes. The vote
    /// may carry the proposal or settle it.
    ///
    /// # Errors
    /// [`Refusal::TurnOver`]; [`Refusal::NoSuchProposal`]; [`Refusal::ProposalClosed`];
    /// [`Refusal::AlreadyVoted`]; a store failure.
    pub fn react_office_in(
        &self,
        run: RunId,
        proposal: i64,
        yes: bool,
        why: &str,
    ) -> Done<Standing> {
        let _held = self.held();
        let turn = self.running(run)?;
        let who = self.member(turn.member)?;
        let found = self.office_proposal(proposal)?;
        if found.status != Status::Open {
            return Err(EngineError::Refused(Refusal::ProposalClosed(proposal)));
        }
        let why = one_line(why, office::WHY_MOST);
        let voted = self.store.add_office_vote(
            proposal,
            &Vote {
                who: who.name.clone(),
                yes,
                why: why.clone(),
                at: self.clock.now(),
            },
        )?;
        if !voted {
            return Err(EngineError::Refused(Refusal::AlreadyVoted));
        }
        self.emit(
            EventKind::OfficeVoted,
            json!({ "proposal": proposal, "by": who.name, "vote": if yes { "yes" } else { "no" },
                    "why": why,
                    "text": format!("{} votes {} on {}'s proposal {proposal}{}", who.name,
                                    if yes { "yes" } else { "no" }, found.by,
                                    if why.is_empty() { String::new() } else { format!(" \u{2014} '{why}'") }) }),
        )?;
        self.moved_forward(run)?;
        self.office_settle(proposal)
    }

    fn office_proposal(&self, id: i64) -> Done<Proposal> {
        self.store
            .office_proposals()?
            .into_iter()
            .find(|p| p.id == id)
            .ok_or(EngineError::Refused(Refusal::NoSuchProposal(id)))
    }

    /// What the votes and the clock say about a proposal now, done (the lock held): it
    /// carries, is dropped, or waits on.
    fn office_settle(&self, id: i64) -> Done<Standing> {
        let found = self.office_proposal(id)?;
        if found.status != Status::Open {
            return Ok(Standing::Open);
        }
        let now = self.clock.now();
        match office::verdict(found.yes(), found.no(), found.at, now) {
            Verdict::Open => Ok(Standing::Open),
            Verdict::Drop(why) => self.office_drop(&found, why.said()),
            Verdict::Apply => {
                let by_votes = found.yes() > found.no() && found.yes() >= office::LEAST_YES;
                self.office_apply(&found, by_votes)
            }
        }
    }

    fn office_drop(&self, found: &Proposal, why: &str) -> Done<Standing> {
        self.store
            .set_office_proposal(found.id, Status::Dropped, why, self.clock.now())?;
        self.emit(
            EventKind::OfficeDropped,
            json!({ "proposal": found.id, "by": found.by, "why": why,
                    "text": format!("Proposal {} ({}'s) is dropped: {why}", found.id, found.by) }),
        )?;
        Ok(Standing::Dropped(why.to_owned()))
    }

    /// Makes a carried proposal's changes, if the rules still allow them (the lock held);
    /// otherwise it is dropped and nothing changes.
    fn office_apply(&self, found: &Proposal, by_votes: bool) -> Done<Standing> {
        let items = self.store.office_items()?;
        if let Err(refused) = office::apply(&items, &found.changes) {
            return self.office_drop(found, &format!("it no longer fits: {refused}"));
        }
        let now = self.clock.now();
        let said = self.describe_all(&items, &found.changes).join("; ");
        self.store
            .push_office_history(&format!("{said} ({}'s idea)", found.by), &items, now)?;
        for change in &found.changes {
            match *change {
                Change::Place { kind, x, y } => {
                    self.store.add_office_item(kind, x, y, &found.by, now)?;
                }
                Change::Move { item, x, y } => self.store.move_office_item(item, x, y)?,
                Change::Remove { item } => self.store.remove_office_item(item)?,
            }
        }
        let outcome = if by_votes {
            format!("carried {} to {}", found.yes(), found.no())
        } else {
            "nobody objected in six hours".to_owned()
        };
        self.store
            .set_office_proposal(found.id, Status::Applied, &outcome, now)?;
        self.emit(
            EventKind::OfficeChanged,
            json!({ "proposal": found.id, "by": found.by,
                    "via": if by_votes { "votes" } else { "quiet" },
                    "text": format!("The office changed: {said} ({}'s idea; {outcome})", found.by) }),
        )?;
        Ok(Standing::Applied)
    }

    /// The first thing each tick does for the office: proposals whose time came carry or are
    /// dropped (the lock held).
    pub(super) fn office_tick(&self) -> Done<()> {
        for open in self
            .store
            .office_proposals()?
            .into_iter()
            .filter(|p| p.status == Status::Open)
        {
            self.office_settle(open.id)?;
        }
        Ok(())
    }

    /// What a turn sees of the office: the things on the floor with their numbers, and the
    /// open proposals.
    ///
    /// # Errors
    /// [`Refusal::TurnOver`]; a store failure.
    pub fn look_office_in(&self, run: RunId) -> Done<String> {
        let _held = self.held();
        let turn = self.running(run)?;
        let who = self.member(turn.member)?;
        self.office_said(&who)
    }

    /// The office in words for a member: its things, numbered, and the open proposals.
    fn office_said(&self, member: &Member) -> Done<String> {
        let items = self.store.office_items()?;
        let mut told = format!(
            "The office is {} tiles across (x) and {} down (y); the top {} rows are the back wall. \
             {} things on the floor (at most {}): ",
            office::COLS,
            office::ROWS,
            office::WALL_ROWS,
            items.len(),
            office::MOST_ITEMS
        );
        let listed: Vec<String> = items
            .iter()
            .take(BRIEFED_ITEMS)
            .map(|i| {
                format!(
                    "{} #{} ({}, {},{})",
                    i.kind.label(),
                    i.id,
                    office::area(i.x, i.y),
                    i.x,
                    i.y
                )
            })
            .collect();
        told.push_str(&listed.join("; "));
        told.push_str(".\n");
        let open: Vec<Proposal> = self
            .store
            .office_proposals()?
            .into_iter()
            .filter(|p| p.status == Status::Open)
            .collect();
        if open.is_empty() {
            told.push_str("No proposals are open.\n");
        }
        for p in &open {
            let voted = p.votes.iter().find(|v| v.who == member.name);
            told.push_str(&format!(
                "Proposal {} by {}: {}{} (yes {}, no {}; {})\n",
                p.id,
                p.by,
                self.describe_all(&items, &p.changes).join("; "),
                if p.why.is_empty() {
                    String::new()
                } else {
                    format!(" \u{2014} '{}'", p.why)
                },
                p.yes(),
                p.no(),
                match voted {
                    Some(v) => format!("you voted {}", if v.yes { "yes" } else { "no" }),
                    None => "you have not voted".to_owned(),
                }
            ));
        }
        Ok(told)
    }

    // ---- the owner as landlord ----------------------------------------------------------

    /// The owner undoes the last change to the office: the things on the floor are as they
    /// were before it. Returns what was undone, or nothing if nothing was left to undo.
    ///
    /// # Errors
    /// A store failure.
    pub fn undo_office(&self) -> Done<Option<String>> {
        let _held = self.held();
        let Some((what, before)) = self.store.pop_office_history()? else {
            return Ok(None);
        };
        self.store.replace_office_items(&before)?;
        self.emit(
            EventKind::OfficeChanged,
            json!({ "by": "owner", "via": "owner_undo", "text": format!("You undid: {what}") }),
        )?;
        Ok(Some(what))
    }

    /// The owner resets the office to the layout it was seeded with. It can itself be undone.
    ///
    /// # Errors
    /// A store failure.
    pub fn reset_office(&self) -> Done<()> {
        let _held = self.held();
        let items = self.store.office_items()?;
        self.store
            .push_office_history("the owner reset the office", &items, self.clock.now())?;
        self.lay_out_the_default()?;
        self.emit(
            EventKind::OfficeChanged,
            json!({ "by": "owner", "via": "owner_reset",
                    "text": "You reset the office to the layout it started with" }),
        )
    }

    /// The owner vetoes an open proposal: it is closed at once, and nothing changes.
    ///
    /// # Errors
    /// [`Refusal::NoSuchProposal`]; [`Refusal::ProposalClosed`]; a store failure.
    pub fn veto_office(&self, proposal: i64) -> Done<()> {
        let _held = self.held();
        let found = self.office_proposal(proposal)?;
        if found.status != Status::Open {
            return Err(EngineError::Refused(Refusal::ProposalClosed(proposal)));
        }
        self.store.set_office_proposal(
            proposal,
            Status::Vetoed,
            "the owner vetoed it",
            self.clock.now(),
        )?;
        self.emit(
            EventKind::OfficeVetoed,
            json!({ "proposal": proposal, "by": "owner",
                    "text": format!("You vetoed {}'s proposal {proposal}", found.by) }),
        )
    }

    // ---- downtime ----------------------------------------------------------------------

    /// What the owner allows paid members' downtime turns together, in cents a day. Nothing,
    /// until the owner says.
    ///
    /// # Errors
    /// A store failure.
    pub fn downtime_paid_cents(&self) -> Done<i64> {
        Ok(self
            .store
            .setting(DOWNTIME_PAID)?
            .and_then(|said| said.parse().ok())
            .unwrap_or(0))
    }

    /// The owner sets what paid members' downtime turns may spend together in a day: none, or
    /// up to what the pool's day holds.
    ///
    /// # Errors
    /// [`Refusal::DowntimeOverPool`]; a store failure.
    pub fn set_downtime_paid_cents(&self, cents: i64) -> Done<()> {
        let _held = self.held();
        let most = money::pool_a_day(self.settings.pool_per_month) / money::CENT;
        if cents < 0 || cents > most {
            return Err(EngineError::Refused(Refusal::DowntimeOverPool {
                most_cents: most,
            }));
        }
        Ok(self.store.set_setting(DOWNTIME_PAID, &cents.to_string())?)
    }

    /// The wakes that were for downtime.
    pub(super) fn downtime_wakes(&self) -> Done<HashSet<WakeId>> {
        Ok(self
            .store
            .wakes()?
            .into_iter()
            .filter(|w| w.reason == WakeReason::Downtime)
            .map(|w| w.id)
            .collect())
    }

    /// Whether a turn is a downtime turn.
    ///
    /// # Errors
    /// A store failure.
    pub fn in_downtime(&self, run: RunId) -> Done<bool> {
        let Some(found) = self.store.run(run)? else {
            return Ok(false);
        };
        Ok(self.downtime_wakes()?.contains(&found.wake))
    }

    fn downtimes(&self) -> Done<Downtimes> {
        let wakes = self.downtime_wakes()?;
        let today = self.today();
        let offset = self.settings.offset_minutes;
        let mut seen = Downtimes {
            live: false,
            last: HashMap::new(),
            today: 0,
        };
        for run in self.store.runs()? {
            if !wakes.contains(&run.wake) {
                continue;
            }
            seen.live |= run.status.live();
            let at = run.ended_at.or(run.started_at).unwrap_or(run.queued_at);
            let last = seen.last.entry(run.member).or_insert(at);
            *last = (*last).max(at);
            if money::day_of(run.queued_at, offset) == today {
                seen.today += if run.status.live() {
                    run.reserved.max(run.spent)
                } else {
                    run.spent
                };
            }
        }
        Ok(seen)
    }

    /// What a downtime turn of `member` may reserve: nothing on the house's model, which is
    /// free; for a paid member only what is left of the owner's allowance for today and of
    /// the pool's day, none if the owner allows none.
    pub(super) fn downtime_reservation(&self, member: &Member) -> Done<Option<Micros>> {
        if !member.brain.cloud() {
            return Ok(Some(0));
        }
        let allowed = self.downtime_paid_cents()? * money::CENT;
        let left = allowed - self.downtimes()?.today;
        Ok(money::reservation(
            left,
            self.pool_left_today()?,
            money::most_a_turn(member.role),
        ))
    }

    /// The look at who has nothing to do (the lock held): at most one member, once a day, is
    /// woken for a downtime turn, and only one such turn runs at a time. A member qualifies
    /// when it is at work and not the CEO, has no live turn, no open task and no routine due,
    /// had no downtime turn today and none in the last twelve hours, and thinks with the
    /// house's model, or with Claude when the owner's allowance for paid downtime has room.
    /// Returns how many were woken.
    pub(super) fn downtime_look(&self, now: Millis) -> Done<usize> {
        let today = self.today().to_string();
        let members = self.store.members()?;
        let mut waiting: Vec<&Member> = Vec::new();
        for member in members
            .iter()
            .filter(|m| m.role != Role::Ceo && m.at_work())
        {
            if self.store.setting(&downtime_day(member.id))?.as_deref() != Some(today.as_str()) {
                waiting.push(member);
            }
        }
        if waiting.is_empty() {
            return Ok(0);
        }
        let seen = self.downtimes()?;
        if seen.live {
            return Ok(0);
        }
        let tasks = self.store.tasks()?;
        let runs = self.store.runs()?;
        let routines = self.store.routines()?;
        waiting.sort_by_key(|m| (seen.last.get(&m.id).copied().unwrap_or(Millis::MIN), m.id));
        for member in waiting {
            let busy = runs
                .iter()
                .any(|r| r.member == member.id && r.status.live());
            let holds = tasks.iter().any(|t| {
                !t.status.finished()
                    && (t.owner == Some(member.id) || t.assignee == Some(member.id))
            });
            let due = routines
                .iter()
                .any(|r| r.enabled && r.assignee == member.id && r.next_due_at <= now);
            let rested = seen
                .last
                .get(&member.id)
                .is_none_or(|at| now - at >= DOWNTIME_REST_MS);
            if busy || holds || due || !rested {
                continue;
            }
            if self.downtime_reservation(member)?.is_none() {
                continue;
            }
            self.store.set_setting(&downtime_day(member.id), &today)?;
            if self.wake_held_lock(member.id, None, WakeReason::Downtime)?
                == crate::domain::Receipt::Queued
            {
                return Ok(1);
            }
        }
        Ok(0)
    }

    /// What a downtime turn is told, after who it is and its notebook: that it is downtime
    /// and makes no work, what the office has, the open proposals, what may be placed and the
    /// rules, and what it may do. Kept short.
    pub(super) fn downtime_said(&self, member: &Member) -> Done<String> {
        let mut told = String::from(
            "It is downtime: you have no task, and nothing is due. This turn is about no task and \
             makes no work (create_task and hand_to are refused).\n",
        );
        told.push_str(&self.office_said(member)?);
        let kinds: Vec<String> = office::CATALOG
            .iter()
            .map(|s| match s.most {
                Some(most) => format!("{} (at most {most})", s.name),
                None => s.name.to_owned(),
            })
            .collect();
        told.push_str(&format!(
            "You may place: {}. Poster, string_lights, clock and art hang on the back wall (y 1); \
             the rest stand on the floor. A change must be inside the floor, and not on a wall, a \
             door, the desks, a walking lane or a room, nor on another thing; the floor holds at \
             most {} things, and a proposal makes at most {} changes.\n",
            kinds.join(", "),
            office::MOST_ITEMS,
            office::MOST_CHANGES
        ));
        told.push_str(&format!(
            "Use the turn well, briefly: react to a proposal (office, action react, yes or no), \
             propose one small change (office, action propose, with why), or note a team idea in \
             your notebook (note, citing event {}). Two yes carry a proposal; with no \"no\" it \
             carries after six hours. Then stop.\n",
            self.store.last_event()?
        ));
        Ok(told)
    }

    // ---- what the screen shows ------------------------------------------------------------

    /// The office as the HQ screen shows it: the things, the catalog, the open proposals and
    /// the last few settled, whether the owner can undo, and the allowance for paid downtime.
    pub(super) fn office_shown(&self) -> Done<serde_json::Value> {
        let items = self.store.office_items()?;
        let proposals = self.store.office_proposals()?;
        let open: Vec<serde_json::Value> = proposals
            .iter()
            .filter(|p| p.status == Status::Open)
            .map(|p| {
                json!({
                    "id": p.id, "by": p.by, "why": p.why, "at": p.at,
                    "changes": self.describe_all(&items, &p.changes),
                    "votes": p.votes.iter().map(|v| json!({
                        "who": v.who, "vote": if v.yes { "yes" } else { "no" }, "why": v.why,
                    })).collect::<Vec<_>>(),
                    "yes": p.yes(), "no": p.no(),
                    "carries_at": p.at + office::QUIET_AFTER_MS,
                    "expires_at": p.at + office::EXPIRES_AFTER_MS,
                })
            })
            .collect();
        let recent: Vec<serde_json::Value> = proposals
            .iter()
            .rev()
            .filter(|p| p.status != Status::Open)
            .take(RECENT_PROPOSALS)
            .map(|p| {
                json!({
                    "id": p.id, "by": p.by, "status": p.status.name(), "outcome": p.outcome,
                    "changes": self.describe_all(&items, &p.changes),
                })
            })
            .collect();
        Ok(json!({
            "cols": office::COLS, "rows": office::ROWS,
            "items": items.iter().map(|i| {
                let spec = i.kind.spec();
                json!({ "id": i.id, "kind": spec.name, "x": i.x, "y": i.y, "w": spec.w,
                        "h": spec.h, "wall": spec.wall, "placed_by": i.placed_by,
                        "placed_at": i.placed_at })
            }).collect::<Vec<_>>(),
            "catalog": office::CATALOG.iter().map(|s| json!({
                "kind": s.name, "w": s.w, "h": s.h, "wall": s.wall, "most": s.most,
            })).collect::<Vec<_>>(),
            "proposals": open,
            "recent": recent,
            "can_undo": self.store.office_history_len()? > 0,
            "downtime_paid_cents_per_day": self.downtime_paid_cents()?,
            "downtime_paid_cents_used": self.downtimes()?.today / money::CENT,
        }))
    }
}

/// The owner's day a member last had a downtime turn queued on.
fn downtime_day(member: MemberId) -> String {
    format!("downtime_day:{}", member.0)
}
