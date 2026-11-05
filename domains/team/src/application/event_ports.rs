//! What the engine needs from the store for the event feed: what happened, in order, for the
//! screen's stream, the scorecard and the record.

use super::ports::Stored;
use crate::domain::{Event, Millis};

/// The event feed's part of HQ's store.
pub trait EventStore {
    /// Appends an event to the feed and returns its id.
    fn emit(&self, at: Millis, kind: &str, data: &serde_json::Value) -> Stored<i64>;
    /// The id of the latest event, 0 when there is none.
    fn last_event(&self) -> Stored<i64>;
    /// How many events of a kind there are, and when the latest was.
    fn count_events(&self, kind: &str) -> Stored<(i64, Option<Millis>)>;
    /// Deletes all but the newest `keep` events that are not ticks, and every tick not among the
    /// newest `keep` of all: the feed is for display, and a reader that fell that far behind
    /// starts again from a snapshot. The ticks do not count, or the heartbeat pushes out the
    /// week the guards read. Endora's audit log is the record.
    fn prune_events(&self, keep: i64) -> Stored<usize>;
    /// The event with this id, if the feed still has it.
    fn event(&self, id: i64) -> Stored<Option<Event>>;
    /// Whether the feed still has the event with this id.
    fn has_event(&self, id: i64) -> Stored<bool>;
    /// The events after `id`, oldest first.
    fn events_after(&self, id: i64) -> Stored<Vec<Event>>;
    /// Every event of these kinds, oldest first.
    fn events_of(&self, kinds: &[&str]) -> Stored<Vec<Event>>;
    /// When the oldest event kept was, if there is one.
    fn first_event_at(&self) -> Stored<Option<Millis>>;
    /// The latest `limit` events with an id below `id`, oldest first.
    fn events_before(&self, id: i64, limit: usize) -> Stored<Vec<Event>>;
}
