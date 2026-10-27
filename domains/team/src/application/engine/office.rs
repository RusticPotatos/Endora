//! The office (ADR 0119, amended by 0142): the floor as data, seeded once, which the HQ screen
//! draws; and downtime, a turn about no task for a member with nothing to do, for a chess move
//! that is due. The team no longer changes the floor: proposals and votes went with the company.

use std::collections::{HashMap, HashSet};

use serde_json::json;

use super::{Done, Engine, EngineError};
use crate::application::ports::{Clock, TeamStore, WorkerAdapter};
use crate::domain::office;
use crate::domain::{Member, MemberId, Millis, Refusal, Role, RunId, WakeId, WakeReason};

/// Set once HQ has put the default layout on the floor, so it never does again.
const OFFICE_SEEDED: &str = "office_seeded";
/// How long a member rests between downtime turns, so none follows another across midnight.
const DOWNTIME_REST_MS: Millis = 12 * 60 * 60 * 1000;

/// What the downtime turns so far say.
pub(super) struct Downtimes {
    /// Whether a downtime turn is queued or running now.
    pub(super) live: bool,
    /// When each member's last downtime turn ended (or began, while it runs).
    pub(super) last: HashMap<MemberId, Millis>,
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

    // ---- deciding together --------------------------------------------------------------

    // ---- the owner as landlord ----------------------------------------------------------

    // ---- downtime ----------------------------------------------------------------------

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

    pub(super) fn downtimes(&self) -> Done<Downtimes> {
        let wakes = self.downtime_wakes()?;
        let mut seen = Downtimes {
            live: false,
            last: HashMap::new(),
        };
        for run in self.store.runs()? {
            if !wakes.contains(&run.wake) {
                continue;
            }
            seen.live |= run.status.live();
            let at = run.ended_at.or(run.started_at).unwrap_or(run.queued_at);
            let last = seen.last.entry(run.member).or_insert(at);
            *last = (*last).max(at);
        }
        Ok(seen)
    }

    /// The look at who has nothing to do (the lock held): at most one member, once a day, is
    /// woken for a downtime turn, and only one such turn runs at a time. A member qualifies
    /// when it is at work and not the CEO, has no live turn, no open task and no routine due,
    /// and had no downtime turn today and none in the last twelve hours. A downtime turn is
    /// work on the NAS's own model (ADR 0131): it costs nothing, and takes nobody's share.
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
        let wakes = self.store.wakes()?;
        waiting.sort_by_key(|m| (seen.last.get(&m.id).copied().unwrap_or(Millis::MIN), m.id));
        for member in waiting {
            // A wake held behind another member's turn is work coming (a huddle's view, a
            // lead's decision or direction, ADR 0120): the member is not idle.
            let busy = runs
                .iter()
                .any(|r| r.member == member.id && r.status.live())
                || wakes
                    .iter()
                    .any(|w| w.member == member.id && w.receipt == crate::domain::Receipt::Held);
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
    /// and makes no work, what the office has and how much of each kind, what the member
    /// placed itself, the open proposals, what may be placed and the rules, and what it may
    /// do, leaving the office alone among it. Kept short.
    pub(super) fn downtime_said(
        &self,
        member: &Member,
        _run: crate::domain::RunId,
    ) -> Done<String> {
        let mut told = String::from(
            "It is downtime: this turn is about no task, no task is due in it, and it makes no \
             work (create_task and hand_to are refused).\n",
        );
        if let Some(move_due) = self.chess_move_said(member)? {
            told.push_str(&move_due);
            return Ok(told);
        }
        told.push_str(&self.chess_briefed(member)?);
        // The nightly quiz on its map, if tonight's was asked of this member (ADR 0126).
        told.push_str(&self.quiz_said(member)?);
        told.push_str("Nothing else is asked of you: stop.\n");
        Ok(told)
    }
    // ---- what the screen shows ------------------------------------------------------------

    /// The office as the HQ screen shows it: the things, the catalog, the open proposals and
    /// the last few settled, and whether the owner can undo.
    pub(super) fn office_shown(&self) -> Done<serde_json::Value> {
        let items = self.store.office_items()?;
        Ok(json!({
            "cols": office::COLS, "rows": office::ROWS,
            "items": items.iter().map(|i| {
                let spec = i.kind.spec();
                json!({ "id": i.id, "kind": spec.name, "x": i.x, "y": i.y, "w": spec.w,
                        "h": spec.h, "wall": spec.wall, "placed_by": i.placed_by,
                        "placed_at": i.placed_at })
            }).collect::<Vec<_>>(),
        }))
    }
}

/// The owner's day a member last had a downtime turn queued on.
pub(super) fn downtime_day(member: MemberId) -> String {
    format!("downtime_day:{}", member.0)
}
