//! The steps of `features/claudes-monthly-credit.feature`: Claude's credit as the node keeps
//! it (`app/node/src/api/credits.rs`), from the owner's reading of Anthropic's Console and what
//! Endora has paid since, with the rules of [`credits`]: what is paid comes off the grant
//! first, and on its day the grant is whole again.

use cucumber::{given, then, when};
use endora_capabilities::credits::{self, DAY_MS, Pots};

use super::Bridge;

/// What the node keeps of Claude's credit, and the owner's clock.
#[derive(Debug, Default)]
pub struct Credit {
    /// The owner's offset from UTC, in minutes.
    offset: i64,
    /// The plan's monthly grant, in micros, and the day it renews.
    grant: i64,
    renews_on: u32,
    /// The figure the estimate was last set to, when, how much of it was bought, and what
    /// Endora had paid by then.
    set: i64,
    set_at: i64,
    purchased_at_set: i64,
    spent_at_set: i64,
    /// What Endora has paid Anthropic in all.
    spent: i64,
    /// Now, by the scenario's clock.
    now: i64,
    /// How many times the plan has renewed the estimate.
    renewals: u32,
}

impl Credit {
    /// The owner's offset from UTC, in minutes, which the lab's scenarios read too.
    pub(super) const fn offset(&self) -> i64 {
        self.offset
    }

    /// `date` (`YYYY-MM-DD`) at `hour:minute` on the owner's clock, in UTC milliseconds: the
    /// one clock the credit's and the lab's scenarios read.
    pub(super) fn at(&self, date: &str, hour: i64, minute: i64) -> i64 {
        let day = day_of(date);
        day * DAY_MS + (hour * 60 + minute - self.offset) * 60_000
    }

    /// As the node does before it shows the credit: when the plan has renewed since the
    /// estimate was set, the estimate starts again from the whole grant and the bought credit
    /// left.
    fn renew_if_due(&mut self) {
        if self.set_at == 0
            || !credits::renewal_due(self.set_at, self.now, self.offset, self.renews_on)
        {
            return;
        }
        let (set, purchased) = credits::renew(self.grant, self.pots());
        self.set = set;
        self.purchased_at_set = purchased;
        self.set_at = self.now;
        self.spent_at_set = self.spent;
        self.renewals += 1;
    }

    fn pots(&self) -> Pots {
        credits::claude_pots(
            self.set,
            self.purchased_at_set,
            self.spent_at_set,
            self.spent,
        )
    }
}

/// The day (from 1970-01-01) of a date written `YYYY-MM-DD`.
fn day_of(date: &str) -> i64 {
    let parts: Vec<&str> = date.split('-').collect();
    let [year, month, day] = parts[..] else {
        panic!("a date is YYYY-MM-DD: {date}");
    };
    credits::days_from_civil(
        year.parse().expect("a year"),
        month.parse().expect("a month"),
        day.parse().expect("a day"),
    )
}

/// Dollars as the owner writes them, in micros: `0` is nothing.
fn dollars(written: &str) -> i64 {
    endora_kernel::money::read_dollars(written).unwrap_or_else(|| {
        assert_eq!(written, "0", "dollars");
        0
    })
}

#[given(regex = r"^the owner's clock is (\d+) hours (behind|ahead of) UTC$")]
fn owners_clock(w: &mut Bridge, hours: i64, side: String) {
    w.credit.offset = if side == "behind" { -hours } else { hours } * 60;
}

#[given(regex = r"^Claude's plan grants \$(\d+) a month, renewing on day (\d+)$")]
fn plan_grants(w: &mut Bridge, grant: String, renews_on: u32) {
    w.credit.grant = dollars(&grant);
    w.credit.renews_on = renews_on;
}

/// The owner's reading of the Console, entered at noon on the owner's clock.
#[given(
    regex = r"^on (\d{4}-\d{2}-\d{2}) the owner read \$(\d+) on Anthropic's Console, \$(\d+) of it bought$"
)]
fn owner_read(w: &mut Bridge, date: String, read: String, bought: String) {
    let at = w.credit.at(&date, 12, 0);
    let credit = &mut w.credit;
    credit.now = at;
    credit.set = dollars(&read);
    credit.set_at = at;
    credit.purchased_at_set = dollars(&bought).min(credit.set);
    credit.spent_at_set = credit.spent;
}

#[given(regex = r"^Endora pays Claude \$(\d+)$")]
#[when(regex = r"^Endora pays Claude \$(\d+)$")]
fn endora_pays(w: &mut Bridge, paid: String) {
    w.credit.renew_if_due();
    w.credit.spent += dollars(&paid);
}

#[when(regex = r"^it is (\d{4}-\d{2}-\d{2}) at (\d{2}):(\d{2})$")]
fn it_is(w: &mut Bridge, date: String, hour: i64, minute: i64) {
    w.credit.now = w.credit.at(&date, hour, minute);
    w.credit.renew_if_due();
}

#[then(regex = r"^\$(\d+) of the grant is left, and \$(\d+) of the bought credit$")]
fn left(w: &mut Bridge, grant: String, bought: String) {
    w.credit.renew_if_due();
    assert_eq!(
        w.credit.pots(),
        Pots {
            grant: dollars(&grant),
            purchased: dollars(&bought),
        }
    );
}

#[then(regex = r"^the plan has (not )?renewed$")]
fn has_renewed(w: &mut Bridge, not: String) {
    assert_eq!(w.credit.renewals > 0, not.is_empty(), "{:?}", w.credit);
}

#[then(regex = r"^after (\d{4}-\d{2}-\d{2}) the plan next renews on (\d{4}-\d{2}-\d{2})$")]
fn next_renews(w: &mut Bridge, after: String, wanted: String) {
    let (year, month, day) =
        credits::civil_from_days(credits::next_renewal(day_of(&after), w.credit.renews_on));
    assert_eq!(format!("{year:04}-{month:02}-{day:02}"), wanted);
}
