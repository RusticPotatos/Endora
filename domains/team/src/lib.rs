//! Endora HQ's team context (ADR 0102): a team of workers taking a goal forward while the
//! owner watches and decides. Goals and tasks, claiming, wakes and their receipts, runs, the
//! tick, the budget ledger and the event feed, with a simulated runner for tests and the demo.
//!
//! Self-contained on purpose: HQ is its own service with its own database, and talks to the
//! Endora node only over its API.

#![forbid(unsafe_code)]
// Pedantic Clippy holds here, and the gate keeps it (team-code-care-2026-10-27). What it asks
// that this code does not do, it does not ask:
#![warn(clippy::pedantic, clippy::excessive_nesting)]
#![allow(
    // The return type says a store can fail; the sections only repeated it (the tidy, step 5).
    clippy::missing_errors_doc,
    // Owned arguments read plainer than borrows the engine's call sites would then take.
    clippy::needless_pass_by_value,
    // `push_str(&format!(..))` builds briefings line by line, as they read.
    clippy::format_push_string,
    // Names like `run` and `ran`, `wake` and `woke` are the domain's words.
    clippy::similar_names,
    // "SQLite", "HQ" and the like are words in prose, not code.
    clippy::doc_markdown,
    // A clear `match` on an option reads as well as `if let … else`.
    clippy::single_match_else,
    // A cloned field assigned whole reads plainer than `clone_from`.
    clippy::assigning_clones,
    // Methods that keep the engine's shape though they read nothing of it.
    clippy::unused_self,
    // Settings and facts are flags by nature.
    clippy::struct_excessive_bools,
    // Arms kept apart that say different things, though they do the same.
    clippy::match_same_arms,
    // Money, tokens and ratings stay far inside every numeric range they are cast across.
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    // Ratings compared as the league computed them.
    clippy::float_cmp,
    // Iterators the name generator bounds with `find`, never driven to the end.
    clippy::maybe_infinite_iter,
    // A store method that returns a result like its neighbours, though it cannot fail.
    clippy::unnecessary_wraps,
    clippy::case_sensitive_file_extension_comparisons,
)]
// Tests may be long and take what they build by value.
#![cfg_attr(test, allow(clippy::too_many_lines, clippy::manual_midpoint))]

pub mod application;
pub mod domain;
pub mod infrastructure;
