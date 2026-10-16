//! Reading the copy on the NAS, and keeping the digest.
//!
//! The copy is a read-only mount: nothing here writes to it, renames in it, or marks anything.
//! A message is parsed for its facts; links are recorded by where they go and replaced in the
//! text; attachments are described and never opened.

use std::path::{Path, PathBuf};

use endora_kernel::RepositoryError;
use endora_persistence::{Db, backend};
use mail_parser::{MessageParser, MimeHeaders};
use rusqlite::params;

use crate::domain::{
    Attachment, Auth, Bucket, Flags, Headline, Link, Message, Reason, Verdict, without_links,
};

/// Largest message read. Anything bigger is mostly attachment, and its facts are not worth
/// reading a hundred megabytes for.
pub const MOST_BYTES: u64 = 25 * 1024 * 1024;

/// Longest text kept from a body; the reader is given less.
const KEPT_TEXT_CHARS: usize = 8000;

// ---- the copy ----------------------------------------------------------------------------

/// The read-only Maildir copy: one folder per account, each with `new` and `cur`.
pub struct MaildirCopy {
    root: PathBuf,
}

impl MaildirCopy {
    /// The copy at `root`.
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Every message's key (`account/file`). An account folder or file name that is not plain
    /// is skipped, so a key can never point outside the copy.
    #[must_use]
    pub fn keys(&self) -> Vec<String> {
        let plain = |s: &str| !s.is_empty() && !s.starts_with('.') && !s.contains(['/', '\\']);
        let mut keys = Vec::new();
        let Ok(accounts) = std::fs::read_dir(&self.root) else {
            return keys;
        };
        for account in accounts.flatten() {
            let Some(name) = account.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if !plain(&name) || !account.path().is_dir() {
                continue;
            }
            for sub in ["new", "cur"] {
                let Ok(files) = std::fs::read_dir(account.path().join(sub)) else {
                    continue;
                };
                for file in files.flatten() {
                    if let Some(file) = file.file_name().to_str() {
                        if plain(file) {
                            keys.push(format!("{name}/{file}"));
                        }
                    }
                }
            }
        }
        keys.sort();
        keys
    }

    /// The message a key names, parsed. `None` if it is gone, too big, or not mail.
    #[must_use]
    pub fn read(&self, key: &str) -> Option<Message> {
        let (account, file) = key.split_once('/')?;
        if file.contains(['/', '\\']) || account.starts_with('.') || file.starts_with('.') {
            return None;
        }
        let path = ["new", "cur"]
            .iter()
            .map(|sub| self.root.join(account).join(sub).join(file))
            .find(|p| p.is_file())?;
        if std::fs::metadata(&path).ok()?.len() > MOST_BYTES {
            return None;
        }
        let bytes = std::fs::read(&path).ok()?;
        parse(key, account, &bytes)
    }

    /// Where the copy is.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }
}

/// The facts of one message.
#[must_use]
pub fn parse(key: &str, account: &str, bytes: &[u8]) -> Option<Message> {
    let parsed = MessageParser::default().parse(bytes)?;
    let lower = |s: &str| s.trim().to_ascii_lowercase();
    let addresses = |a: Option<&mail_parser::Address<'_>>| -> Vec<String> {
        a.map(|a| a.iter().filter_map(|x| x.address()).map(lower).collect())
            .unwrap_or_default()
    };
    let from = parsed.from().and_then(|a| a.first());
    let header = |name: &'static str| {
        parsed
            .header_raw(name)
            .map(str::trim)
            .filter(|s| !s.is_empty())
    };
    let text_list = |v: &mail_parser::HeaderValue<'_>| -> Vec<String> {
        v.as_text_list()
            .map(|l| {
                l.iter()
                    .map(|s| format!("<{}>", s.trim_matches(['<', '>'])))
                    .collect()
            })
            .or_else(|| {
                v.as_text()
                    .map(|s| vec![format!("<{}>", s.trim_matches(['<', '>']))])
            })
            .unwrap_or_default()
    };
    let mut replies_to = text_list(parsed.in_reply_to());
    replies_to.extend(text_list(parsed.references()));
    replies_to.sort();
    replies_to.dedup();

    let precedence = header("Precedence").map(lower).unwrap_or_default();
    let mailing_list = header("List-Unsubscribe").is_some()
        || header("List-Id").is_some()
        || matches!(precedence.as_str(), "bulk" | "list" | "junk");

    let html = parsed
        .body_html(0)
        .map(|h| h.into_owned())
        .unwrap_or_default();
    let plain = parsed
        .body_text(0)
        .map(|t| t.into_owned())
        .unwrap_or_default();
    let mut links = links_in_html(&html);
    links.extend(links_in_text(&plain));
    links.sort_by(|a, b| (&a.domain, &a.text).cmp(&(&b.domain, &b.text)));
    links.dedup();
    links.truncate(50);

    let attachments = parsed
        .attachments()
        // An invitation is recorded as one, below; it is not a file someone sent.
        .filter(|part| !part.is_content_type("text", "calendar"))
        .take(20)
        .map(|part| Attachment {
            name: part
                .attachment_name()
                .unwrap_or("(unnamed)")
                .chars()
                .take(120)
                .collect(),
            mime: part
                .content_type()
                .map(|t| format!("{}/{}", t.ctype(), t.subtype().unwrap_or("")))
                .unwrap_or_default(),
            size: part.len(),
        })
        .collect();
    let invitation = parsed
        .parts
        .iter()
        .any(|p| p.is_content_type("text", "calendar"));

    let text: String = without_links(&plain)
        .chars()
        .take(KEPT_TEXT_CHARS)
        .collect();
    Some(Message {
        key: key.to_owned(),
        account: account.to_owned(),
        at_ms: parsed.date().map_or(0, |d| d.to_timestamp() * 1000),
        from_address: from
            .and_then(|f| f.address())
            .map(lower)
            .unwrap_or_default(),
        from_name: from
            .and_then(|f| f.name())
            .map(|n| n.trim().chars().take(120).collect())
            .unwrap_or_default(),
        reply_to: addresses(parsed.reply_to()).into_iter().next(),
        to: addresses(parsed.to()),
        cc: addresses(parsed.cc()),
        delivered_to: header("Delivered-To").map(lower),
        subject: parsed.subject().unwrap_or("").chars().take(300).collect(),
        message_id: parsed
            .message_id()
            .map(|id| format!("<{}>", id.trim_matches(['<', '>'])))
            .unwrap_or_default(),
        replies_to,
        mailing_list,
        auth: auth_in(parsed.header_raw("Authentication-Results").unwrap_or("")),
        links,
        attachments,
        invitation,
        text,
    })
}

/// The receiving server's SPF, DKIM and DMARC verdicts.
#[must_use]
pub fn auth_in(results: &str) -> Auth {
    let lower = results.to_ascii_lowercase();
    let verdict = |name: &str| {
        let key = format!("{name}=");
        lower.find(&key).map_or(Verdict::Unknown, |at| {
            let word: String = lower[at + key.len()..]
                .chars()
                .take_while(char::is_ascii_alphabetic)
                .collect();
            match word.as_str() {
                "pass" => Verdict::Pass,
                "fail" | "softfail" | "permerror" => Verdict::Fail,
                _ => Verdict::Unknown,
            }
        })
    };
    Auth {
        spf: verdict("spf"),
        dkim: verdict("dkim"),
        dmarc: verdict("dmarc"),
    }
}

/// The host a URL really goes to. `https://example.com@example.net/` goes to `example.net`,
/// not `example.com`, which is the trick this exists to see through.
#[must_use]
pub fn host_of(url: &str) -> Option<String> {
    let rest = url.trim().split_once("://").map_or(url.trim(), |(_, r)| r);
    let authority = rest.split(['/', '?', '#']).next()?;
    let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let host = host
        .split(':')
        .next()?
        .trim_end_matches('.')
        .to_ascii_lowercase();
    (host.contains('.')
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-.".contains(c)))
    .then_some(host)
}

fn links_in_text(text: &str) -> Vec<Link> {
    text.split_whitespace()
        .map(|w| w.trim_matches(|c: char| "()<>[]\"',.;".contains(c)))
        .filter(|w| w.starts_with("http://") || w.starts_with("https://"))
        .filter_map(|w| {
            Some(Link {
                text: String::new(),
                domain: host_of(w)?,
            })
        })
        .collect()
}

/// `<a href="...">text</a>`: where each link goes, and what it says it is. A small scanner,
/// not a browser: it only has to find the anchors, and a malformed one is skipped.
fn links_in_html(html: &str) -> Vec<Link> {
    let lower = html.to_ascii_lowercase();
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(start) = lower[at..].find("<a ").map(|i| at + i) {
        let Some(tag_end) = lower[start..].find('>').map(|i| start + i) else {
            break;
        };
        let tag = &html[start..tag_end];
        let close = lower[tag_end..]
            .find("</a")
            .map_or(tag_end + 1, |i| tag_end + i);
        at = close.max(tag_end + 1);
        let Some(href) = attribute(tag, "href") else {
            continue;
        };
        let Some(domain) = host_of(&href) else {
            continue;
        };
        let shown: String = strip_tags(html.get(tag_end + 1..close).unwrap_or(""));
        out.push(Link {
            text: shown
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .chars()
                .take(120)
                .collect(),
            domain,
        });
    }
    out
}

fn attribute(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let at = lower.find(&format!("{name}="))? + name.len() + 1;
    let rest = &tag[at..];
    let (quote, rest) = match rest.chars().next()? {
        q @ ('"' | '\'') => (Some(q), &rest[1..]),
        _ => (None, rest),
    };
    let value: String = match quote {
        Some(q) => rest.split(q).next()?.to_owned(),
        None => rest
            .split(|c: char| c.is_whitespace() || c == '>')
            .next()?
            .to_owned(),
    };
    Some(value.replace("&amp;", "&"))
}

fn strip_tags(html: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
}

// ---- the digest --------------------------------------------------------------------------

/// Creates the mail tables if absent (idempotent).
///
/// # Errors
/// [`RepositoryError::Backend`] if the schema cannot be applied.
pub fn migrate(db: &Db) -> Result<(), RepositoryError> {
    db.lock()?
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS mail_messages (
                key           TEXT PRIMARY KEY,
                account       TEXT NOT NULL,
                at_ms         INTEGER NOT NULL,
                from_address  TEXT NOT NULL,
                headline      TEXT NOT NULL,
                triaged_ms    INTEGER,
                from_name     TEXT,
                bucket        TEXT,
                reasons       TEXT,
                sentence      TEXT,
                flags         TEXT,
                read_ok       INTEGER
            ) STRICT;
            CREATE INDEX IF NOT EXISTS mail_messages_untriaged
                ON mail_messages (triaged_ms, at_ms);
            CREATE TABLE IF NOT EXISTS mail_meta (
                key   TEXT PRIMARY KEY,
                value TEXT NOT NULL
            ) STRICT;",
        )
        .map_err(backend)?;
    Ok(())
}

/// What Endora keeps about one message once it has been read: no body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Digest {
    /// The message's key.
    pub key: String,
    /// When it was sent.
    pub at_ms: i64,
    /// Who sent it.
    pub from_address: String,
    /// Their name as written.
    pub from_name: String,
    /// Where it was sorted.
    pub bucket: Bucket,
    /// Why it matters; empty for most.
    pub reasons: Vec<Reason>,
    /// The reader's sentence, checked; `None` if it gave none that survived.
    pub sentence: Option<String>,
    /// What the reader flagged, checked.
    pub flags: Flags,
    /// Whether the reader's reply was usable at all.
    pub read_ok: bool,
    /// Reasons it may not be what it says, as a person reads them; kept for the kinds of mail
    /// worth forging (money, security, orders, appointments, the app).
    pub cautions: Vec<String>,
    /// For a bill: the calendar appointment it most likely follows, as `(day, title)`.
    pub appointment: Option<(String, String)>,
}

/// The digest and the index of every message, in Endora's database.
pub struct MailStore {
    db: Db,
}

impl MailStore {
    /// Over the shared connection.
    #[must_use]
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    /// The keys already indexed.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    pub fn known_keys(&self) -> Result<std::collections::HashSet<String>, RepositoryError> {
        let conn = self.db.lock()?;
        let mut stmt = conn
            .prepare("SELECT key FROM mail_messages")
            .map_err(backend)?;
        let keys = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(backend)?
            .collect::<Result<_, _>>()
            .map_err(backend)?;
        Ok(keys)
    }

    /// Indexes a message's headline. A key already indexed is left as it is.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    pub fn index(&self, m: &Message) -> Result<(), RepositoryError> {
        let h = m.headline();
        let json = serde_json::json!({
            "recipients": h.recipients,
            "delivered_to": h.delivered_to,
            "message_id": h.message_id,
            "replies_to": h.replies_to,
        });
        self.db
            .lock()?
            .execute(
                "INSERT OR IGNORE INTO mail_messages (key, account, at_ms, from_address, headline)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![m.key, m.account, m.at_ms, m.from_address, json.to_string()],
            )
            .map_err(backend)?;
        Ok(())
    }

    /// Every headline indexed: what the mailbox is learned from.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    pub fn headlines(&self) -> Result<Vec<Headline>, RepositoryError> {
        let conn = self.db.lock()?;
        let mut stmt = conn
            .prepare("SELECT key, at_ms, from_address, headline FROM mail_messages")
            .map_err(backend)?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                ))
            })
            .map_err(backend)?;
        let strings = |v: &serde_json::Value| -> Vec<String> {
            v.as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|s| s.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default()
        };
        let mut out = Vec::new();
        for row in rows {
            let (key, at_ms, from_address, json) = row.map_err(backend)?;
            let v: serde_json::Value = serde_json::from_str(&json).unwrap_or_default();
            out.push(Headline {
                key,
                at_ms,
                from_address,
                recipients: strings(&v["recipients"]),
                delivered_to: v["delivered_to"].as_str().map(str::to_owned),
                message_id: v["message_id"].as_str().unwrap_or("").to_owned(),
                replies_to: strings(&v["replies_to"]),
            });
        }
        Ok(out)
    }

    /// Messages not yet read, sent at or after `since_ms`, newest first.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    pub fn unread_since(
        &self,
        since_ms: i64,
        limit: usize,
    ) -> Result<Vec<String>, RepositoryError> {
        let conn = self.db.lock()?;
        let mut stmt = conn
            .prepare(
                "SELECT key FROM mail_messages WHERE triaged_ms IS NULL AND at_ms >= ?1
                 ORDER BY at_ms DESC LIMIT ?2",
            )
            .map_err(backend)?;
        let keys = stmt
            .query_map(
                params![since_ms, i64::try_from(limit).unwrap_or(i64::MAX)],
                |r| r.get::<_, String>(0),
            )
            .map_err(backend)?
            .collect::<Result<_, _>>()
            .map_err(backend)?;
        Ok(keys)
    }

    /// Records what was made of a message.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    pub fn save(&self, d: &Digest, now_ms: i64) -> Result<(), RepositoryError> {
        let flags = serde_json::json!({
            "money_due": d.flags.money_due,
            "amount": d.flags.amount,
            "account_change": d.flags.account_change,
            "one_time_code": d.flags.one_time_code,
            "order_problem": d.flags.order_problem,
            "needs_reply": d.flags.needs_reply,
            "dates": d.flags.dates.iter().map(|(date, what)| serde_json::json!({"date": date, "what": what})).collect::<Vec<_>>(),
            "cautions": d.cautions,
            "appointment": d.appointment.as_ref().map(|(day, title)| serde_json::json!({"day": day, "title": title})),
        });
        let reasons: Vec<&str> = d.reasons.iter().map(|r| r.name()).collect();
        self.db
            .lock()?
            .execute(
                "UPDATE mail_messages SET triaged_ms = ?2, from_name = ?3, bucket = ?4,
                 reasons = ?5, sentence = ?6, flags = ?7, read_ok = ?8 WHERE key = ?1",
                params![
                    d.key,
                    now_ms,
                    d.from_name,
                    d.bucket.name(),
                    serde_json::to_string(&reasons).unwrap_or_default(),
                    d.sentence,
                    flags.to_string(),
                    i64::from(d.read_ok),
                ],
            )
            .map_err(backend)?;
        Ok(())
    }

    /// Marks a message as needing no reading: the owner's own, or one that could not be
    /// parsed. It is never offered to the reader again.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    pub fn pass_over(&self, key: &str, now_ms: i64) -> Result<(), RepositoryError> {
        self.db
            .lock()?
            .execute(
                "UPDATE mail_messages SET triaged_ms = ?2, read_ok = 0 WHERE key = ?1",
                params![key, now_ms],
            )
            .map_err(backend)?;
        Ok(())
    }

    /// A small stored fact about the reading, such as which version of the reader's
    /// instructions the digest was made with.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    pub fn meta(&self, key: &str) -> Result<Option<String>, RepositoryError> {
        use rusqlite::OptionalExtension;
        self.db
            .lock()?
            .query_row(
                "SELECT value FROM mail_meta WHERE key = ?1",
                params![key],
                |r| r.get::<_, String>(0),
            )
            .optional()
            .map_err(backend)
    }

    /// Stores one of those facts.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    pub fn set_meta(&self, key: &str, value: &str) -> Result<(), RepositoryError> {
        self.db
            .lock()?
            .execute(
                "INSERT INTO mail_meta (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )
            .map_err(backend)?;
        Ok(())
    }

    /// Forgets what was made of mail sent at or after `since_ms`, so it is read again. The
    /// index (who, to whom, which thread) is kept; only the reading goes.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    pub fn forget_readings_since(&self, since_ms: i64) -> Result<usize, RepositoryError> {
        let changed = self
            .db
            .lock()?
            .execute(
                "UPDATE mail_messages SET triaged_ms = NULL, from_name = NULL, bucket = NULL,
                 reasons = NULL, sentence = NULL, flags = NULL, read_ok = NULL
                 WHERE at_ms >= ?1 AND triaged_ms IS NOT NULL",
                params![since_ms],
            )
            .map_err(backend)?;
        Ok(changed)
    }

    /// What was made of the mail sent at or after `since_ms`, newest first.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    pub fn digests_since(&self, since_ms: i64) -> Result<Vec<Digest>, RepositoryError> {
        let conn = self.db.lock()?;
        let mut stmt = conn
            .prepare(
                "SELECT key, at_ms, from_address, from_name, bucket, reasons, sentence, flags, read_ok
                 FROM mail_messages WHERE bucket IS NOT NULL AND at_ms >= ?1 ORDER BY at_ms DESC",
            )
            .map_err(backend)?;
        let rows = stmt
            .query_map(params![since_ms], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, Option<String>>(5)?,
                    r.get::<_, Option<String>>(6)?,
                    r.get::<_, Option<String>>(7)?,
                    r.get::<_, Option<i64>>(8)?,
                ))
            })
            .map_err(backend)?;
        let mut out = Vec::new();
        for row in rows {
            let (key, at_ms, from_address, from_name, bucket, reasons, sentence, flags, read_ok) =
                row.map_err(backend)?;
            let Some(bucket) = Bucket::from_name(&bucket) else {
                continue;
            };
            let reasons: Vec<String> =
                serde_json::from_str(reasons.as_deref().unwrap_or("[]")).unwrap_or_default();
            let f: serde_json::Value =
                serde_json::from_str(flags.as_deref().unwrap_or("{}")).unwrap_or_default();
            out.push(Digest {
                key,
                at_ms,
                from_address,
                from_name: from_name.unwrap_or_default(),
                bucket,
                reasons: reasons.iter().filter_map(|r| reason_named(r)).collect(),
                sentence,
                flags: Flags {
                    money_due: f["money_due"].as_bool().unwrap_or(false),
                    amount: f["amount"].as_str().map(str::to_owned),
                    account_change: f["account_change"].as_bool().unwrap_or(false),
                    one_time_code: f["one_time_code"].as_bool().unwrap_or(false),
                    order_problem: f["order_problem"].as_bool().unwrap_or(false),
                    needs_reply: f["needs_reply"].as_bool().unwrap_or(false),
                    dates: f["dates"]
                        .as_array()
                        .map(|a| {
                            a.iter()
                                .filter_map(|d| {
                                    Some((
                                        d["date"].as_str()?.to_owned(),
                                        d["what"].as_str()?.to_owned(),
                                    ))
                                })
                                .collect()
                        })
                        .unwrap_or_default(),
                },
                read_ok: read_ok == Some(1),
                cautions: f["cautions"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|c| c.as_str().map(str::to_owned))
                            .collect()
                    })
                    .unwrap_or_default(),
                appointment: f["appointment"]["day"]
                    .as_str()
                    .zip(f["appointment"]["title"].as_str())
                    .map(|(d, t)| (d.to_owned(), t.to_owned())),
            });
        }
        Ok(out)
    }

    /// How many messages are indexed, and how many of the last `since_ms` are still unread.
    ///
    /// # Errors
    /// [`RepositoryError`] if the backend fails.
    pub fn progress(&self, since_ms: i64) -> Result<(usize, usize), RepositoryError> {
        let conn = self.db.lock()?;
        let count = |sql: &str, p: &[&dyn rusqlite::ToSql]| -> Result<usize, RepositoryError> {
            conn.query_row(sql, p, |r| r.get::<_, i64>(0))
                .map(|n| usize::try_from(n).unwrap_or(0))
                .map_err(backend)
        };
        Ok((
            count("SELECT count(*) FROM mail_messages", &[])?,
            count(
                "SELECT count(*) FROM mail_messages WHERE triaged_ms IS NULL AND at_ms >= ?1",
                &[&since_ms],
            )?,
        ))
    }
}

fn reason_named(name: &str) -> Option<Reason> {
    [
        Reason::ReplyInYourThread,
        Reason::FromSomeoneYouWriteTo,
        Reason::WaitingOnYou,
        Reason::MoneyDue,
        Reason::AccountSecurity,
        Reason::Appointment,
        Reason::OrderProblem,
        Reason::YourApp,
        Reason::MayNotBeGenuine,
    ]
    .into_iter()
    .find(|r| r.name() == name)
}

#[cfg(test)]
mod tests;
