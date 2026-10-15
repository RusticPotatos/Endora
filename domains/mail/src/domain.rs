//! What code can tell about a message, and how that sorts it (ADR 0097).
//!
//! Pure: no file, no database, no model. A message reaches here as the facts its headers and
//! MIME tree carry; whatever a model later says about it is checked against these rules, never
//! the other way round.

use std::collections::{BTreeMap, BTreeSet};

mod caution;
mod sorting;
mod text;

pub use caution::{Caution, cautions, guarded_sentence, written_dates};
pub use sorting::{Bucket, Flags, Reason, bucket, why_important};
pub use sorting::{days_of, read_reply};
pub use text::{for_the_reader, safe_sentence, without_links};

/// How long a real person can wait before "you have not answered" counts as important.
pub const WAITING_AFTER_MS: i64 = 3 * 24 * 60 * 60 * 1000;

/// One message, as code reads it. Built by the infrastructure from the copy on the NAS.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Message {
    /// The copy's own name for it (`account/file`): stable, and how the digest refers to it.
    pub key: String,
    /// Which mailbox it came from.
    pub account: String,
    /// When it was sent.
    pub at_ms: i64,
    /// Who sent it, lowercased.
    pub from_address: String,
    /// The sender's display name, as written.
    pub from_name: String,
    /// Where replies would go, lowercased, if it says.
    pub reply_to: Option<String>,
    /// Who it was sent to, lowercased.
    pub to: Vec<String>,
    /// Who was copied, lowercased.
    pub cc: Vec<String>,
    /// The mailbox it was delivered to, which is how the owner's address is known.
    pub delivered_to: Option<String>,
    /// Its subject.
    pub subject: String,
    /// Its `Message-ID`.
    pub message_id: String,
    /// The messages it answers (`In-Reply-To` and `References`).
    pub replies_to: Vec<String>,
    /// Sent through a mailing list or a bulk sender: `List-Unsubscribe`, `List-Id` or
    /// `Precedence: bulk|list|junk`.
    pub mailing_list: bool,
    /// What the receiving server's authentication checks said.
    pub auth: Auth,
    /// Every link, by the text shown and the domain it really goes to.
    pub links: Vec<Link>,
    /// Attachments: names, types and sizes only. Never opened.
    pub attachments: Vec<Attachment>,
    /// It carries a calendar invitation.
    pub invitation: bool,
    /// The readable text, links replaced, bounded. Only ever given to the quarantined reader.
    pub text: String,
}

/// The receiving server's verdicts, from `Authentication-Results`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Auth {
    /// SPF.
    pub spf: Verdict,
    /// DKIM.
    pub dkim: Verdict,
    /// DMARC.
    pub dmarc: Verdict,
}

/// One authentication check's result.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Verdict {
    /// It passed.
    Pass,
    /// It failed, or the sender's domain says to reject or quarantine.
    Fail,
    /// The header does not say.
    #[default]
    Unknown,
}

/// A link: the text a reader sees and the domain it actually goes to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    /// What the message shows.
    pub text: String,
    /// The host the link points at, lowercased.
    pub domain: String,
}

/// An attachment, described and never opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attachment {
    /// Its file name, as the sender gave it.
    pub name: String,
    /// Its declared type.
    pub mime: String,
    /// Its size in bytes.
    pub size: usize,
}

/// The small facts kept for every message, however old: enough to know whom the owner writes
/// to, which threads are theirs, and whether a sender is new. No text.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Headline {
    /// See [`Message::key`].
    pub key: String,
    /// See [`Message::at_ms`].
    pub at_ms: i64,
    /// See [`Message::from_address`].
    pub from_address: String,
    /// Recipients, `to` and `cc` together.
    pub recipients: Vec<String>,
    /// See [`Message::delivered_to`].
    pub delivered_to: Option<String>,
    /// See [`Message::message_id`].
    pub message_id: String,
    /// See [`Message::replies_to`].
    pub replies_to: Vec<String>,
}

impl Message {
    /// The facts kept for it in the index.
    #[must_use]
    pub fn headline(&self) -> Headline {
        Headline {
            key: self.key.clone(),
            at_ms: self.at_ms,
            from_address: self.from_address.clone(),
            recipients: self.to.iter().chain(&self.cc).cloned().collect(),
            delivered_to: self.delivered_to.clone(),
            message_id: self.message_id.clone(),
            replies_to: self.replies_to.clone(),
        }
    }
}

/// What the whole mailbox says about the people in it, learned from every headline.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Mailbox {
    /// The owner's own addresses: where mail is delivered.
    pub owners: BTreeSet<String>,
    /// Everyone the owner has sent mail to.
    pub written_to: BTreeSet<String>,
    /// The `Message-ID`s of mail the owner sent: their threads.
    pub sent_ids: BTreeSet<String>,
    /// The `Message-ID`s the owner has replied to.
    pub answered: BTreeSet<String>,
    /// Per sending domain: the first time it was seen and how many messages since.
    pub domains: BTreeMap<String, (i64, usize)>,
}

impl Mailbox {
    /// Learns it from every headline there is.
    #[must_use]
    pub fn learn(headlines: &[Headline]) -> Self {
        let mut delivered: BTreeMap<&str, usize> = BTreeMap::new();
        for h in headlines {
            if let Some(to) = &h.delivered_to {
                *delivered.entry(to.as_str()).or_default() += 1;
            }
        }
        // Each account's own address is where its mail arrives. More than one account means
        // more than one; a stray `Delivered-To` seen once or twice is not an owner.
        let busiest = delivered.values().copied().max().unwrap_or(0);
        let owners: BTreeSet<String> = delivered
            .into_iter()
            .filter(|(_, n)| *n * 20 >= busiest && *n >= 3)
            .map(|(address, _)| address.to_owned())
            .collect();
        let mut mailbox = Self {
            owners,
            ..Self::default()
        };
        for h in headlines {
            if mailbox.owners.contains(&h.from_address) {
                mailbox.written_to.extend(
                    h.recipients
                        .iter()
                        .filter(|r| !mailbox.owners.contains(*r))
                        .cloned(),
                );
                if !h.message_id.is_empty() {
                    mailbox.sent_ids.insert(h.message_id.clone());
                }
                mailbox.answered.extend(h.replies_to.iter().cloned());
                continue;
            }
            let entry = mailbox
                .domains
                .entry(domain_of(&h.from_address))
                .or_insert((h.at_ms, 0));
            entry.0 = entry.0.min(h.at_ms);
            entry.1 += 1;
        }
        mailbox
    }

    /// Whether the owner sent it.
    #[must_use]
    pub fn sent_by_owner(&self, m: &Message) -> bool {
        self.owners.contains(&m.from_address)
    }

    /// Whether its sender is someone the owner has written to.
    #[must_use]
    pub fn from_someone_written_to(&self, m: &Message) -> bool {
        self.written_to.contains(&m.from_address)
    }

    /// Whether it answers mail the owner sent.
    #[must_use]
    pub fn in_owners_thread(&self, m: &Message) -> bool {
        m.replies_to.iter().any(|id| self.sent_ids.contains(id))
    }

    /// Whether the owner has replied to it.
    #[must_use]
    pub fn answered(&self, m: &Message) -> bool {
        !m.message_id.is_empty() && self.answered.contains(&m.message_id)
    }
}

/// The domain part of an address, lowercased.
#[must_use]
pub fn domain_of(address: &str) -> String {
    address
        .rsplit_once('@')
        .map_or(address, |(_, domain)| domain)
        .trim()
        .trim_end_matches('>')
        .to_ascii_lowercase()
}

/// The part of a domain that someone registered: `billing.example.co.uk` is `example.co.uk`.
/// A short list of two-part public suffixes covers the common cases; anything else is the last
/// two labels.
#[must_use]
pub fn registered_domain(domain: &str) -> String {
    const TWO_PART: &[&str] = &[
        "co.uk", "org.uk", "ac.uk", "gov.uk", "com.au", "net.au", "org.au", "co.nz", "co.jp",
        "com.br", "com.mx", "co.in", "co.za", "com.cn", "com.sg",
    ];
    let labels: Vec<&str> = domain.trim_end_matches('.').split('.').collect();
    if labels.len() <= 2 {
        return labels.join(".");
    }
    let last_two = labels[labels.len() - 2..].join(".");
    if TWO_PART.contains(&last_two.as_str()) {
        return labels[labels.len() - 3..].join(".");
    }
    last_two
}

#[cfg(test)]
mod tests;
