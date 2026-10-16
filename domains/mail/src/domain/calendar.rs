//! What the owner's calendar says about their mail (ADR 0097): whether a bill follows an
//! appointment with whoever sent it, and which dates mentioned in mail are real enough to show.
//!
//! Calendar entries reach here as `(YYYY-MM-DD, title)`, already read. Matching is by shared
//! words, which is crude and is named as crude: it can only ever add a line of evidence or put
//! a date on a list, never decide anything.

use std::collections::{BTreeMap, BTreeSet};

use super::{days_of, domain_of};

/// Days before a bill to look for the appointment it is for. Medical and dental bills often
/// come weeks after the visit.
pub const BILL_LOOKBACK_DAYS: i64 = 60;

/// Days after a bill's date to still count an appointment (a bill sent ahead of a booking).
pub const BILL_LOOKAHEAD_DAYS: i64 = 14;

/// Words too common in sender names to say who someone is.
const NOT_A_NAME: &[&str] = &[
    "billing",
    "bills",
    "bill",
    "statement",
    "statements",
    "account",
    "accounts",
    "payment",
    "payments",
    "team",
    "services",
    "service",
    "noreply",
    "reply",
    "info",
    "mail",
    "email",
    "support",
    "notification",
    "notifications",
    "customer",
    "care",
    "office",
    "center",
    "centre",
    "your",
    "from",
    "with",
    "group",
    "company",
    "online",
    "alerts",
    "alert",
    "news",
    "hello",
    "admin",
    "example",
];

/// The words that say who sent something: from its display name and its domain.
#[must_use]
pub fn sender_words(from_name: &str, from_address: &str) -> BTreeSet<String> {
    let domain = domain_of(from_address);
    let label = super::registered_domain(&domain);
    let label = label.split('.').next().unwrap_or("");
    words(&format!("{from_name} {label}"))
}

fn words(text: &str) -> BTreeSet<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.chars().count() >= 4 && !NOT_A_NAME.contains(w))
        .map(str::to_owned)
        .collect()
}

fn shares_a_word(a: &BTreeSet<String>, title: &str) -> bool {
    let theirs = words(title);
    a.iter().any(|w| theirs.contains(w))
}

/// The appointment a bill sent on `bill_day` is most likely for: an entry from
/// [`BILL_LOOKBACK_DAYS`] before to [`BILL_LOOKAHEAD_DAYS`] after, sharing a word with the
/// sender. The nearest wins.
#[must_use]
pub fn appointment_for(
    from_name: &str,
    from_address: &str,
    bill_day: i64,
    entries: &[(String, String)],
) -> Option<(String, String)> {
    let who = sender_words(from_name, from_address);
    if who.is_empty() {
        return None;
    }
    entries
        .iter()
        .filter_map(|(day, title)| Some((days_of(day)?, day, title)))
        .filter(|(d, ..)| {
            *d >= bill_day - BILL_LOOKBACK_DAYS && *d <= bill_day + BILL_LOOKAHEAD_DAYS
        })
        .filter(|(_, _, title)| shares_a_word(&who, title))
        .min_by_key(|(d, ..)| (d - bill_day).abs())
        .map(|(_, day, title)| (day.clone(), title.clone()))
}

/// A date one message mentions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mention {
    /// The day, `YYYY-MM-DD`.
    pub date: String,
    /// What happens then, in the reader's few words.
    pub what: String,
    /// Who the message was from, as shown.
    pub from: String,
    /// Which message, so two mentions in one message do not count as two.
    pub key: String,
    /// From someone the owner writes to, or in their thread, and nothing casts doubt on it.
    pub trusted: bool,
}

/// A date worth showing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Upcoming {
    /// The day.
    pub date: String,
    /// What happens then.
    pub what: String,
    /// Who mentioned it.
    pub from: String,
    /// Why it is believed, as a person reads it.
    pub because: &'static str,
}

/// The dates from mail worth showing, soonest first, and how many were held back.
///
/// A date is shown only when something confirms it: someone the owner writes to said it, two
/// or more messages say it, or an earlier calendar entry shares a word with it. A date already
/// on the calendar that day is not repeated. Past dates are dropped. Everything else is held
/// back and only counted, so an "act by Friday!" from a stranger never becomes a fact.
#[must_use]
pub fn coming_up(
    mentions: &[Mention],
    entries: &[(String, String)],
    today: i64,
) -> (Vec<Upcoming>, usize) {
    let mut by_date: BTreeMap<&str, Vec<&Mention>> = BTreeMap::new();
    for m in mentions {
        if days_of(&m.date).is_some_and(|d| d >= today) {
            by_date.entry(m.date.as_str()).or_default().push(m);
        }
    }
    let mut shown = Vec::new();
    let mut held = 0;
    for (date, said) in by_date {
        let about: BTreeSet<String> = said
            .iter()
            .flat_map(|m| words(&format!("{} {}", m.what, m.from)))
            .collect();
        let already_on = entries
            .iter()
            .any(|(day, title)| day == date && shares_a_word(&about, title));
        if already_on {
            continue;
        }
        let messages: BTreeSet<&str> = said.iter().map(|m| m.key.as_str()).collect();
        let earlier_entry = entries.iter().any(|(day, title)| {
            days_of(day).is_some_and(|d| d < today) && shares_a_word(&about, title)
        });
        let because = if said.iter().any(|m| m.trusted) {
            Some("from someone you write to")
        } else if messages.len() >= 2 {
            Some("in more than one message")
        } else if earlier_entry {
            Some("matches something already on your calendar")
        } else {
            None
        };
        match because {
            Some(because) => {
                let first = said.iter().find(|m| m.trusted).unwrap_or(&said[0]);
                shown.push(Upcoming {
                    date: date.to_owned(),
                    what: first.what.clone(),
                    from: first.from.clone(),
                    because,
                });
            }
            None => held += 1,
        }
    }
    (shown, held)
}
