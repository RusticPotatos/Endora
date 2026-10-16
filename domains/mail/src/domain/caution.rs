//! Whether a message may not be what it says it is, from facts code can check (ADR 0097).
//!
//! A bill or a security notice is exactly what someone would forge. These checks read the
//! receiving server's verdicts and compare names; they never decide a message *is* genuine,
//! only collect the reasons it might not be, for the person to weigh.

use super::{Mailbox, Message, Verdict, domain_of, registered_domain};

/// Messages from a domain after which it is an established sender, not a possible forger.
const ESTABLISHED_AFTER: usize = 2;

/// One reason for caution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Caution {
    /// The receiving server says the sender's checks failed.
    SenderChecksFailed,
    /// Neither DKIM nor DMARC passed: nothing vouches for the sender.
    SenderNotVouchedFor,
    /// Replies would go to a different domain from the sender's, from a new sender or one DMARC
    /// does not vouch for.
    RepliesGoElsewhere(String),
    /// Replies would go elsewhere, from an established sender DMARC vouches for: worth knowing,
    /// and ordinary for large senders.
    RepliesElsewhereKnownSender(String),
    /// The sender's domain is a near miss for one the owner already hears from.
    LooksLike(String),
    /// The first message ever seen from this domain.
    NewSender,
    /// Its links go to domains other than the sender's.
    LinksGoElsewhere(Vec<String>),
}

impl Caution {
    /// Whether it is a reason to doubt the message, rather than only something to know.
    #[must_use]
    pub const fn serious(&self) -> bool {
        matches!(
            self,
            Self::SenderChecksFailed
                | Self::SenderNotVouchedFor
                | Self::RepliesGoElsewhere(_)
                | Self::LooksLike(_)
        )
    }

    /// How a person reads it.
    #[must_use]
    pub fn said(&self) -> String {
        match self {
            Self::SenderChecksFailed => "the sender's checks failed".to_owned(),
            Self::SenderNotVouchedFor => "nothing vouches for the sender".to_owned(),
            Self::RepliesGoElsewhere(d) | Self::RepliesElsewhereKnownSender(d) => {
                format!("replies go to {d}")
            }
            Self::LooksLike(d) => format!("the sender's domain looks like {d} but is not"),
            Self::NewSender => "the first message from this sender".to_owned(),
            Self::LinksGoElsewhere(d) => format!("links go to {}", d.join(", ")),
        }
    }
}

/// Every reason for caution about a message.
#[must_use]
pub fn cautions(m: &Message, mailbox: &Mailbox) -> Vec<Caution> {
    let mut out = Vec::new();
    let a = m.auth;
    // DMARC passing is the verdict that matters: it says the domain in From sent this. SPF on
    // its own fails for ordinary forwarded mail (seen on live mail), so it counts only when
    // DMARC did not pass.
    let failed = a.dmarc == Verdict::Fail
        || (a.dmarc != Verdict::Pass && (a.dkim == Verdict::Fail || a.spf == Verdict::Fail));
    if failed {
        out.push(Caution::SenderChecksFailed);
    } else if a.dkim != Verdict::Pass && a.dmarc != Verdict::Pass {
        out.push(Caution::SenderNotVouchedFor);
    }
    let from_domain = domain_of(&m.from_address);
    let from_registered = registered_domain(&from_domain);
    let heard_from = mailbox.domains.get(&from_domain).map_or(0, |(_, n)| *n);
    // Tuned on the first live run: established senders that DMARC vouches for took replies at
    // another of their own domains, and a sender heard from many times was called a lookalike
    // of another. Both are doubt only where a forger could stand: a new sender, or one DMARC
    // does not vouch for.
    let could_be_forged = a.dmarc != Verdict::Pass || heard_from <= ESTABLISHED_AFTER;
    if let Some(reply_to) = &m.reply_to {
        let reply_registered = registered_domain(&domain_of(reply_to));
        if reply_registered != from_registered {
            out.push(if could_be_forged {
                Caution::RepliesGoElsewhere(reply_registered)
            } else {
                Caution::RepliesElsewhereKnownSender(reply_registered)
            });
        }
    }
    if heard_from <= ESTABLISHED_AFTER {
        if let Some(known) = near_miss(&from_registered, mailbox) {
            out.push(Caution::LooksLike(known));
        }
    }
    if heard_from <= 1 {
        out.push(Caution::NewSender);
    }
    let mut elsewhere: Vec<String> = m
        .links
        .iter()
        .map(|l| registered_domain(&l.domain))
        .filter(|d| *d != from_registered)
        .collect();
    elsewhere.sort();
    elsewhere.dedup();
    if !elsewhere.is_empty() {
        elsewhere.truncate(3);
        out.push(Caution::LinksGoElsewhere(elsewhere));
    }
    out
}

/// A domain the owner already hears from, or writes to, that `registered` is a near miss of:
/// one or two letters different, or the international-letters encoding (`xn--`) of a
/// lookalike.
fn near_miss(registered: &str, mailbox: &Mailbox) -> Option<String> {
    let label = |d: &str| d.split('.').next().unwrap_or("").to_owned();
    let mine = label(registered);
    let mut known: Vec<String> = mailbox
        .domains
        .iter()
        .filter(|(_, (_, n))| *n >= 3)
        .map(|(d, _)| registered_domain(d))
        .chain(
            mailbox
                .written_to
                .iter()
                .map(|a| registered_domain(&domain_of(a))),
        )
        .collect();
    known.sort();
    known.dedup();
    known.into_iter().filter(|k| k != registered).find(|k| {
        let theirs = label(k);
        (registered.starts_with("xn--") && !theirs.is_empty())
            || (theirs.len() >= 4
                && mine != theirs
                && !hyphenated_from(&theirs, &mine)
                && edits(&mine, &theirs) <= allowed(&theirs))
    })
}

/// Whether `longer` is `name` with a hyphenated part added (`e-name`, `name-mail`): a brand's
/// own short name beside the one its mailings come from. An impersonator adds to a name to
/// look like a brand; the shorter one is the brand's, so it is not a near miss of the longer.
fn hyphenated_from(longer: &str, name: &str) -> bool {
    longer.starts_with(&format!("{name}-")) || longer.ends_with(&format!("-{name}"))
}

/// How far off a name may be and still be a near miss: one letter for a short name, where two
/// letters of difference is simply another name (live, a short name was matched against
/// several unrelated ones), two for a longer one.
fn allowed(name: &str) -> usize {
    if name.chars().count() <= 6 { 1 } else { 2 }
}

/// Levenshtein distance, which is all a near miss needs.
fn edits(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut prev = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let here = row[j + 1];
            row[j + 1] = (prev + usize::from(ca != cb)).min(row[j] + 1).min(here + 1);
            prev = here;
        }
    }
    row[b.len()]
}

/// What to show in place of the reader's sentence when a message may not be genuine: written
/// by code from the facts, so a forger's words are never presented as Endora's.
#[must_use]
pub fn guarded_sentence(m: &Message, cautions: &[Caution]) -> String {
    let who = if m.from_name.trim().is_empty() {
        domain_of(&m.from_address)
    } else {
        format!("{} ({})", m.from_name.trim(), domain_of(&m.from_address))
    };
    let why: Vec<String> = cautions
        .iter()
        .filter(|c| c.serious())
        .map(Caution::said)
        .collect();
    format!("Says it is from {who}; {}.", why.join("; "))
}

/// Keeps only the dates whose day of the month is written in the message, so a date the model
/// worked out itself ("Thursday", "this Friday") and may have got wrong is not kept as fact.
#[must_use]
pub fn written_dates(dates: Vec<(String, String)>, m: &Message) -> Vec<(String, String)> {
    let text = format!("{} {}", m.subject, m.text).to_ascii_lowercase();
    let words: Vec<&str> = text
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    dates
        .into_iter()
        .filter(|(date, _)| {
            let Some(day) = date.get(8..10).and_then(|d| d.parse::<u32>().ok()) else {
                return false;
            };
            let forms = [
                day.to_string(),
                format!("{day:02}"),
                format!("{day}st"),
                format!("{day}nd"),
                format!("{day}rd"),
                format!("{day}th"),
            ];
            words.iter().any(|w| forms.iter().any(|f| f == w)) || text.contains(date.as_str())
        })
        .collect()
}
