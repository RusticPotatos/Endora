//! A member out for the day (ADR 0113, amended): a paid member may spend at most twice its
//! share (`money::may_draw`). When one is turned away for that while the pool's day still has
//! money, one item goes on the owner's desk; approving gives it an allowance for the rest of the
//! owner's day, which is gone when the next one starts. Capped again after an allowance, it is
//! asked about again (amended 2026-11-01, D21); declined, it is not. Also today's pool: its
//! base, sized from the Claude credit the node says (ADR 0113, amended 2026-10-31), and the
//! owner's top-up.

use serde_json::json;

use super::{Done, Engine, EngineError};
use crate::application::ports::{Clock, DeskItem, TeamStore, WorkerAdapter};
use crate::domain::credit_pool::{self, CreditWord, PoolDay};
use crate::domain::projects::{dollars, read_dollars};
use crate::domain::{EventKind, Member, MemberId, Micros, Refusal, money};

/// What the owner gave a member for one day. The day is in the key, so it ends with the day.
fn allowance_key(member: MemberId, day: i64) -> String {
    format!("allowance:{}:{day}", member.0)
}

/// What the owner topped the pool up by for one day. The day is in the key, so it ends with
/// the day, as an allowance does.
fn topup_key(day: i64) -> String {
    format!("pool_topup:{day}")
}

/// What the node said of the Claude credit on one owner's day. The day is in the key, so the
/// next day is sized afresh.
fn credit_key(day: i64) -> String {
    format!("pool_credit:{day}")
}

/// That a member has already asked the owner for one day, and what its allowance was then.
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

    /// What the owner has added to the pool for the owner's day `day`, if anything.
    pub(super) fn topup_of(&self, day: i64) -> Done<Micros> {
        Ok(self
            .store
            .setting(&topup_key(day))?
            .and_then(|said| said.parse().ok())
            .unwrap_or(0))
    }

    /// Today's pool: the base day (sized from the Claude credit, ADR 0113 amended 2026-10-31)
    /// and the owner's top-up for today (ADR 0113 rule 6). Every member's share is its claim's
    /// part of this, so a larger base or a top-up grows each share, and the borrowing cap with
    /// it, in proportion.
    pub(super) fn pool_day_today(&self) -> Done<Micros> {
        Ok(self.pool_sized_today()?.base + self.topup_of(self.today())?)
    }

    /// The node says what is left of the Claude plan's grant and the day the plan next renews
    /// (`YYYY-MM-DD` on the owner's calendar), or `None` for either when there is no grant (ADR
    /// 0113, amended 2026-10-31). The first grant heard in an owner's day sizes that day's pool,
    /// and later words that day change nothing: what the team spends today must not shrink
    /// today's pool under it. A day heard to have no grant takes a grant heard later in it. A
    /// renewal that is not after today is out of date and is not kept.
    ///
    /// # Errors
    /// A store failure.
    pub fn tell_claude_credit(&self, left: Option<Micros>, renews: Option<&str>) -> Done<()> {
        let _held = self.held();
        let day = self.today();
        let word = match (left, renews.and_then(credit_pool::day_of_date)) {
            (Some(left), Some(renews)) if renews > day => CreditWord::Grant { left, renews },
            (Some(_), Some(_)) => return Ok(()),
            _ => CreditWord::NoGrant,
        };
        let key = credit_key(day);
        let kept = self
            .store
            .setting(&key)?
            .and_then(|said| CreditWord::read(&said));
        if matches!(kept, Some(CreditWord::Grant { .. })) || kept == Some(word) {
            return Ok(());
        }
        self.store.set_setting(&key, &word.kept())?;
        Ok(())
    }

    /// Today's base pool and how it was sized (ADR 0113, amended 2026-10-31): from the Claude
    /// grant the node said today, with the floor's part for members whose brain is not a Claude
    /// kept on top, between the floor and three times it.
    pub(super) fn pool_sized_today(&self) -> Done<PoolDay> {
        let day = self.today();
        let word = self
            .store
            .setting(&credit_key(day))?
            .and_then(|said| CreditWord::read(&said));
        let per_month = self.settings.pool_per_month;
        let (mut paid, mut others) = (0, 0);
        for member in self.store.members()? {
            if member.furloughed() {
                continue;
            }
            let claim = money::claim(member.share_percent, member.brain);
            paid += claim;
            if !member.brain.claude() {
                others += claim;
            }
        }
        let others = money::share_of_day(money::pool_a_day(per_month), others, paid);
        Ok(credit_pool::sized(per_month, others, word, day))
    }

    /// The owner adds `amount` to today's pool, and only today's: the setting is keyed by the
    /// owner's day. Two top-ups in a day add up, and together they are at most five times the
    /// base pool day.
    ///
    /// # Errors
    /// [`Refusal::TopUpOutOfRange`]; a store failure.
    pub fn top_up_pool(&self, amount: Micros) -> Done<Micros> {
        let _held = self.held();
        let day = self.today();
        let so_far = self.topup_of(day)?;
        let room = (money::most_top_up(self.settings.pool_per_month) - so_far).max(0);
        if amount <= 0 || amount > room {
            return Err(EngineError::Refused(Refusal::TopUpOutOfRange { room }));
        }
        let total = so_far + amount;
        self.store
            .set_setting(&topup_key(day), &total.to_string())?;
        self.emit(
            EventKind::PoolToppedUp,
            json!({ "amount": amount, "total": total, "by": "owner", "day": day }),
        )?;
        Ok(total)
    }

    /// A paid member was turned away at twice its share, and its allowance, with the pool's day
    /// not spent: one item for the owner (the lock held). Asked once for each allowance it has
    /// had today (D21 of round 3: Ada, capped again after her allowance with $3.78 in the pool,
    /// stalled with no card until the next day): the first time, and again after each the owner
    /// gives; never while one waits, and not again once the owner has declined.
    pub(super) fn ask_about_the_day(
        &self,
        member: &Member,
        day: i64,
        (share, spent, allowance): (Micros, Micros, Micros),
        pool_left: Micros,
    ) -> Done<()> {
        let key = asked_key(member.id, day);
        let given = allowance.to_string();
        if self
            .store
            .setting(&key)?
            .is_some_and(|asked| asked == given)
        {
            return Ok(());
        }
        self.store.set_setting(&key, &given)?;
        let already = if allowance > 0 {
            format!(", {} already given today", dollars(allowance))
        } else {
            String::new()
        };
        let text = format!(
            "{name} is out for today: {spent} spent (share {share}, borrowed {borrowed}{already}); \
             the pool has {left} left today.\nGive {name} {offer} more today, or wait for \
             tomorrow.",
            name = member.name,
            spent = dollars(spent),
            share = dollars(share),
            borrowed = dollars((spent - share - allowance).max(0)),
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
