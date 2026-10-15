//! # Mail context (ADR 0097)
//!
//! What code can tell about a message, how it is sorted, and the digest Endora keeps. The
//! mail itself is a read-only copy on the NAS; this context reads it, never writes to it, and
//! never opens a link or an attachment. Models only describe, through a quarantined reader the
//! orchestration layer supplies; the rules here decide.

#![forbid(unsafe_code)]

pub mod domain;
pub mod infrastructure;

pub use domain::{
    Attachment, Auth, Bucket, Caution, Flags, Headline, Link, Mailbox, Message, Reason, Verdict,
    bucket, cautions, days_of, domain_of, for_the_reader, guarded_sentence, read_reply,
    registered_domain, safe_sentence, why_important, without_links, written_dates,
};
pub use infrastructure::{Digest, MailStore, MaildirCopy, migrate};
