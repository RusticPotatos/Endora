//! A member out for the day (ADR 0113, amended): a paid member may spend at most twice its
//! share (`money::may_draw`). The first time one is turned away for that while the pool's day
//! still has money, one item goes on the owner's desk; approving gives it an allowance for the
//! rest of the owner's day, which is gone when the next one starts.

use serde_json::json;

use super::{Done, Engine, EngineError};
use crate::application::ports::{Clock, DeskItem, TeamStore, WorkerAdapter};
use crate::domain::projects::{dollars, read_dollars};
use crate::domain::{EventKind, Member, MemberId, Micros, Refusal, money};

/// What the owner gave a member for one day. The day is in the key, so it ends with the day.
fn allowance_key(member: MemberId, day: i64) -> String {
    format!("allowance:{}:{day}", member.0)
}

/// That a member has already asked the owner for one day.
fn asked_key(member: MemberId, day: i64) -> String {
    format!("out_for_day_asked:{}:{day}", member.0)
}

impl<S: TeamStore, W: WorkerAdapter, C: Clock> Engine<S, W, C> {
    /// What the owner has given `member` for the owner's day `day`, if anything.
    pub(super) fn allowance_of(&self, member: MemberId, day: i64) -> Done<Micros> {
        Ok(self
            .store
            .setting(&allowance_key(member, day))?
            .and_then(|said| said.parse().ok())
            .unwrap_or(0))
    }

    /// A paid member was turned away at twice its share with the pool's day not spent: one
    /// item for the owner, once a member a day (the lock held).
    pub(super) fn ask_about_the_day(
        &self,
        member: &Member,
        day: i64,
        share: Micros,
        spent: Micros,
        pool_left: Micros,
    ) -> Done<()> {
        let key = asked_key(member.id, day);
        if self.store.setting(&key)?.is_some() {
            return Ok(());
        }
        self.store.set_setting(&key, "1")?;
        let text = format!(
            "{name} is out for today: {spent} spent (share {share}, borrowed {borrowed}); the \
             pool has {left} left today.\nGive {name} {offer} more today, or wait for tomorrow.",
            name = member.name,
            spent = dollars(spent),
            share = dollars(share),
            borrowed = dollars((spent - share).max(0)),
            left = dollars(pool_left),
            offer = dollars(money::allowance_offer(pool_left)),
        );
        let item = self.store.add_desk(
            "out_for_day",
            None,
            None,
            Some(member.id),
            &text,
            self.clock.now(),
        )?;
        self.emit(
            EventKind::DeskAdded,
            json!({ "item": item.0, "kind": "out_for_day", "member": member.id.0,
                    "name": member.name }),
        )
    }

    /// The owner's word on a member out for the day (the lock held): `approve` gives it
    /// `words` in dollars, or the offer, for the rest of today, never more than the pool's day
    /// has left; `decline` changes nothing.
    pub(super) fn owner_decides_the_day(
        &self,
        found: &DeskItem,
        decision: &str,
        words: &str,
    ) -> Done<()> {
        match decision {
            "decline" => {}
            "approve" => {
                let member = found.member.ok_or(EngineError::Unknown("member"))?;
                let day = self.today();
                let left = self.pool_left_today()?;
                let amount = read_dollars(words).unwrap_or_else(|| money::allowance_offer(left));
                if amount > left || amount < money::LEAST_A_TURN {
                    return Err(EngineError::Refused(Refusal::AllowanceOverPool { left }));
                }
                self.store
                    .set_setting(&allowance_key(member, day), &amount.to_string())?;
            }
            _ => return Err(EngineError::Unknown("decision for this item")),
        }
        self.store.set_desk_status(found.id, decision)?;
        self.emit(
            EventKind::DeskDecided,
            json!({ "item": found.id.0, "kind": found.kind, "decision": decision }),
        )
    }
}
