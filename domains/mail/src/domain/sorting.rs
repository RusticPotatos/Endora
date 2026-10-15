//! Buckets, importance, and what is accepted from the quarantined reader.

use super::{Mailbox, Message, WAITING_AFTER_MS};

/// Where a message is sorted. Endora's own view, never written back to the mailbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Bucket {
    /// A person writing, rather than a company or a machine.
    People,
    /// Bills, invoices, payments, renewals.
    Money,
    /// Orders, shipping and deliveries.
    Orders,
    /// Sign-ins, password and security changes, codes.
    Security,
    /// Appointments, bookings and invitations.
    Appointments,
    /// Newsletters, promotions, anything sent to a list.
    Newsletters,
    /// Notifications and updates that ask nothing of anyone.
    Updates,
    /// The owner's own app: App Store Connect, TestFlight, App Review, and its users.
    YourApp,
    /// Everything else.
    Other,
}

impl Bucket {
    /// Every bucket, in the order a person would read them.
    pub const ALL: [Self; 9] = [
        Self::People,
        Self::Money,
        Self::Security,
        Self::Appointments,
        Self::Orders,
        Self::YourApp,
        Self::Updates,
        Self::Newsletters,
        Self::Other,
    ];

    /// The stable name used in storage and by the reader.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::People => "people",
            Self::Money => "money",
            Self::Orders => "orders",
            Self::Security => "security",
            Self::Appointments => "appointments",
            Self::Newsletters => "newsletters",
            Self::Updates => "updates",
            Self::YourApp => "your_app",
            Self::Other => "other",
        }
    }

    /// The bucket a name means, if any.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|b| b.name() == name.trim())
    }

    /// What a person reads as the bucket's title.
    #[must_use]
    pub const fn title(self) -> &'static str {
        match self {
            Self::People => "People",
            Self::Money => "Money",
            Self::Orders => "Orders and deliveries",
            Self::Security => "Security",
            Self::Appointments => "Appointments",
            Self::Newsletters => "Newsletters and promotions",
            Self::Updates => "Updates",
            Self::YourApp => "Your app",
            Self::Other => "Other",
        }
    }

    /// Mail a company sends about something real: the kinds a model may move a mailing-list
    /// message into, since bills and receipts come from bulk senders too.
    const fn transactional(self) -> bool {
        matches!(
            self,
            Self::Money | Self::Orders | Self::Security | Self::Appointments | Self::YourApp
        )
    }
}

/// What the quarantined reader is allowed to say about a message, after checking.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Flags {
    /// It asks for money: a bill, an invoice, a renewal, a payment due.
    pub money_due: bool,
    /// The amount, as digits and one point (`180.00`), if it gives one.
    pub amount: Option<String>,
    /// It is about the account itself: a new sign-in, a password or security change.
    pub account_change: bool,
    /// Something went wrong with an order or delivery.
    pub order_problem: bool,
    /// It asks the owner something, or waits on them.
    pub needs_reply: bool,
    /// Dates it mentions, as `YYYY-MM-DD` and a few words on what happens then.
    pub dates: Vec<(String, String)>,
}

/// Why a message matters, as code decided it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Reason {
    /// A reply in a thread the owner started.
    ReplyInYourThread,
    /// From someone the owner has written to.
    FromSomeoneYouWriteTo,
    /// A real person, waiting on an answer for three days or more.
    WaitingOnYou,
    /// It asks for money.
    MoneyDue,
    /// It is about the account's security.
    AccountSecurity,
    /// An appointment or an invitation.
    Appointment,
    /// An order or delivery went wrong.
    OrderProblem,
    /// About the owner's app, from something other than a mailing list.
    YourApp,
    /// Money, security, an order, an appointment or the app, from a sender nothing vouches
    /// for: worth a look precisely because it may be a forgery.
    MayNotBeGenuine,
}

impl Reason {
    /// The stable name used in storage.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::ReplyInYourThread => "reply_in_your_thread",
            Self::FromSomeoneYouWriteTo => "from_someone_you_write_to",
            Self::WaitingOnYou => "waiting_on_you",
            Self::MoneyDue => "money_due",
            Self::AccountSecurity => "account_security",
            Self::Appointment => "appointment",
            Self::OrderProblem => "order_problem",
            Self::YourApp => "your_app",
            Self::MayNotBeGenuine => "may_not_be_genuine",
        }
    }

    /// How a person reads it.
    #[must_use]
    pub const fn said(self) -> &'static str {
        match self {
            Self::ReplyInYourThread => "a reply in a thread you started",
            Self::FromSomeoneYouWriteTo => "from someone you write to",
            Self::WaitingOnYou => "waiting on your answer for 3+ days",
            Self::MoneyDue => "asks for money",
            Self::AccountSecurity => "about your account's security",
            Self::Appointment => "an appointment or invitation",
            Self::OrderProblem => "a problem with an order",
            Self::YourApp => "about your app",
            Self::MayNotBeGenuine => "may not be genuine",
        }
    }
}

/// Where a message belongs. Code decides first: an invitation is an appointment; a message
/// from someone the owner writes to, or in the owner's thread, is from a person; anything sent
/// to a list is a newsletter. Only after that does the reader's suggestion count, and it can
/// move a list message into one of the kinds companies really send (a bill, an order), never
/// into *People*.
#[must_use]
pub fn bucket(m: &Message, mailbox: &Mailbox, suggested: Option<Bucket>) -> Bucket {
    if m.invitation {
        return Bucket::Appointments;
    }
    if !m.mailing_list && (mailbox.from_someone_written_to(m) || mailbox.in_owners_thread(m)) {
        return Bucket::People;
    }
    match suggested {
        Some(b) if m.mailing_list && b.transactional() => b,
        _ if m.mailing_list => Bucket::Newsletters,
        Some(b) => b,
        None => Bucket::Other,
    }
}

/// Why it matters, if it does. Empty for most mail, and always for the owner's own and for
/// newsletters.
#[must_use]
pub fn why_important(
    m: &Message,
    mailbox: &Mailbox,
    bucket: Bucket,
    flags: &Flags,
    now_ms: i64,
) -> Vec<Reason> {
    if mailbox.sent_by_owner(m) || bucket == Bucket::Newsletters {
        return Vec::new();
    }
    let mut reasons = Vec::new();
    let known = mailbox.from_someone_written_to(m);
    let thread = mailbox.in_owners_thread(m);
    if bucket == Bucket::People {
        if thread {
            reasons.push(Reason::ReplyInYourThread);
        } else if known {
            reasons.push(Reason::FromSomeoneYouWriteTo);
        }
        if (known || thread)
            && flags.needs_reply
            && !mailbox.answered(m)
            && now_ms.saturating_sub(m.at_ms) >= WAITING_AFTER_MS
        {
            reasons.push(Reason::WaitingOnYou);
        }
    }
    if bucket == Bucket::Money && flags.money_due {
        reasons.push(Reason::MoneyDue);
    }
    if bucket == Bucket::Security && flags.account_change {
        reasons.push(Reason::AccountSecurity);
    }
    if bucket == Bucket::Appointments {
        reasons.push(Reason::Appointment);
    }
    if bucket == Bucket::Orders && flags.order_problem {
        reasons.push(Reason::OrderProblem);
    }
    if bucket == Bucket::YourApp && !m.mailing_list {
        reasons.push(Reason::YourApp);
    }
    reasons
}

/// What the quarantined reader said, kept only if it is exactly the shape asked for: a known
/// bucket, a sentence that survives [`super::safe_sentence`], yes/no flags, an amount that is a
/// number, and dates that are real days within a week before `today_days` and a year after.
/// Anything else in the reply is ignored. `None` if the reply is not JSON at all.
#[must_use]
pub fn read_reply(raw: &str, today_days: i64) -> Option<(Option<Bucket>, Option<String>, Flags)> {
    let start = raw.find('{')?;
    let end = raw.rfind('}')?;
    let value: serde_json::Value = serde_json::from_str(raw.get(start..=end)?).ok()?;
    let object = value.as_object()?;
    let yes = |key: &str| {
        object
            .get(key)
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
    };
    let suggested = object
        .get("bucket")
        .and_then(serde_json::Value::as_str)
        .and_then(Bucket::from_name);
    let sentence = object
        .get("sentence")
        .and_then(serde_json::Value::as_str)
        .and_then(super::safe_sentence);
    let amount = object.get("amount").and_then(|a| match a {
        serde_json::Value::Number(n) => n.as_f64(),
        serde_json::Value::String(s) => s
            .trim()
            .trim_start_matches(['$', '£', '€'])
            .replace(',', "")
            .parse::<f64>()
            .ok(),
        _ => None,
    });
    let amount = amount
        .filter(|a| a.is_finite() && *a > 0.0 && *a < 1_000_000.0)
        .map(|a| format!("{a:.2}"));
    let dates = object
        .get("dates")
        .and_then(serde_json::Value::as_array)
        .map(|all| {
            all.iter()
                .filter_map(|d| {
                    let date = d.get("date")?.as_str()?.trim();
                    let days = days_of(date)?;
                    if days < today_days - 7 || days > today_days + 366 {
                        return None;
                    }
                    let what = super::safe_sentence(d.get("what")?.as_str()?)?;
                    Some((date.to_owned(), what.chars().take(80).collect()))
                })
                .take(3)
                .collect()
        })
        .unwrap_or_default();
    Some((
        suggested,
        sentence,
        Flags {
            money_due: yes("money_due"),
            amount,
            account_change: yes("account_change"),
            order_problem: yes("order_problem"),
            needs_reply: yes("needs_reply"),
            dates,
        },
    ))
}

/// Days since 1970-01-01 for a real `YYYY-MM-DD`, `None` for anything else (including
/// 2026-02-30).
#[must_use]
pub fn days_of(date: &str) -> Option<i64> {
    let mut parts = date.split('-');
    let (y, m, d) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() || y.len() != 4 || m.len() != 2 || d.len() != 2 {
        return None;
    }
    let (y, m, d): (i64, i64, i64) = (y.parse().ok()?, m.parse().ok()?, d.parse().ok()?);
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let month_days = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    if !(1..=12).contains(&m) || d < 1 || d > month_days[(m - 1) as usize] {
        return None;
    }
    // Howard Hinnant's days-from-civil.
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}
