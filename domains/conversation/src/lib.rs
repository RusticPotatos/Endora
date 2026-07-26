//! # Conversation context
//!
//! The chat surface itself: the [`ChatMessage`] model and the [`ChatRepository`]
//! that persists the conversation with the butler. The butler-turn contract
//! (the reply, its proposals and formed beliefs) and the orchestration that runs
//! a turn compose several contexts and so live in the orchestration layer, not
//! here. Depends only on the shared kernel — a leaf context. See ADR 0050.

#![forbid(unsafe_code)]

pub mod application;
pub mod domain;
pub mod infrastructure;

pub use application::ChatRepository;
pub use domain::{ChatMessage, MessageRole};
pub use infrastructure::{ChatStore, migrate};
