//! The agenda loop (ADR 0103): what Endora already reads (the containers, the download
//! queues, the mail it has sorted, the calendar) becomes **signals**; fixed rules turn each
//! into an **item** at one of four levels (handled, FYI, needs you, urgent). The only thing it
//! does unasked is restart a container that crashed or went unhealthy, within caps. A
//! persisting signal is one item, a failed read closes nothing, and Stop teaches a rule.

#![forbid(unsafe_code)]

pub mod application;
pub mod domain;
pub mod infrastructure;
