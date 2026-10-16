//! What the agenda needs from outside: somewhere to keep its items, a way to restart a
//! container, a way to push, and Activity.

use crate::domain::{Family, Kind, Level, Response, Source};
use std::fmt;

/// Why the agenda could not do something.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgendaError(pub String);

impl fmt::Display for AgendaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for AgendaError {}

/// The agenda's result.
pub type Done<T> = Result<T, AgendaError>;

/// One thing the agenda is keeping track of, from when it was seen until it cleared or the
/// owner answered it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    /// Its id; 0 until stored.
    pub id: i64,
    /// What it is, and which one: the same thing seen again is the same item.
    pub key: String,
    /// Where it was seen.
    pub source: Source,
    /// What kind of thing it is.
    pub kind: Kind,
    /// What a taught rule matches.
    pub about: String,
    /// What the owner reads.
    pub sentence: String,
    /// Why it is at its level, or what was done: "Restarted it."
    pub why: String,
    /// How it is told.
    pub level: Level,
    /// What the rules decided when it opened.
    pub response: Response,
    /// For a container, its id.
    pub target: Option<String>,
    /// When it opened.
    pub opened_at: i64,
    /// When it last changed.
    pub updated_at: i64,
    /// While watched: when it is told at `then` if it has not cleared.
    pub watch_until: Option<i64>,
    /// The level it is told at when the watch runs out.
    pub then: Option<Level>,
    /// Whether it has been pushed.
    pub pushed: bool,
    /// When it closed.
    pub closed_at: Option<i64>,
    /// How it closed: cleared, done, dismissed, stopped or expired.
    pub closed_how: Option<String>,
}

/// A taught rule: Stop on an item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    /// Its id.
    pub id: i64,
    /// What it is about.
    pub family: Family,
    /// Which one.
    pub about: String,
    /// When it was taught.
    pub at: i64,
}

/// Where the agenda keeps what it knows.
pub trait AgendaStore {
    /// Every item still open.
    ///
    /// # Errors
    /// The store failed.
    fn open_items(&self) -> Done<Vec<Item>>;
    /// One item.
    ///
    /// # Errors
    /// The store failed.
    fn item(&self, id: i64) -> Done<Option<Item>>;
    /// Whether an item with this key was ever opened.
    ///
    /// # Errors
    /// The store failed.
    fn ever_had(&self, key: &str) -> Done<bool>;
    /// Keeps a new item; returns its id.
    ///
    /// # Errors
    /// The store failed.
    fn add_item(&self, item: &Item) -> Done<i64>;
    /// Saves a changed item.
    ///
    /// # Errors
    /// The store failed.
    fn save_item(&self, item: &Item) -> Done<()>;
    /// Forgets items that closed before a time; returns how many.
    ///
    /// # Errors
    /// The store failed.
    fn prune_closed(&self, before: i64) -> Done<usize>;
    /// The restarts since a time, as `(container, at)`.
    ///
    /// # Errors
    /// The store failed.
    fn restarts_since(&self, since: i64) -> Done<Vec<(String, i64)>>;
    /// Records a restart.
    ///
    /// # Errors
    /// The store failed.
    fn add_restart(&self, container: &str, at: i64) -> Done<()>;
    /// Every taught rule.
    ///
    /// # Errors
    /// The store failed.
    fn rules(&self) -> Done<Vec<Rule>>;
    /// Teaches a rule; returns its id.
    ///
    /// # Errors
    /// The store failed.
    fn add_rule(&self, family: Family, about: &str, at: i64) -> Done<i64>;
    /// Forgets a rule; returns it if there was one.
    ///
    /// # Errors
    /// The store failed.
    fn remove_rule(&self, id: i64) -> Done<Option<Rule>>;
    /// The owner's re-levels.
    ///
    /// # Errors
    /// The store failed.
    fn responses(&self) -> Done<Vec<(Kind, Response)>>;
    /// Re-levels a kind, or puts it back with `None`.
    ///
    /// # Errors
    /// The store failed.
    fn set_response(&self, kind: Kind, response: Option<Response>) -> Done<()>;
    /// How many pushes since a time.
    ///
    /// # Errors
    /// The store failed.
    fn pushes_since(&self, since: i64) -> Done<usize>;
    /// Records a push.
    ///
    /// # Errors
    /// The store failed.
    fn add_push(&self, at: i64) -> Done<()>;
    /// When each source was last read in full.
    ///
    /// # Errors
    /// The store failed.
    fn checked(&self) -> Done<Vec<(Source, i64)>>;
    /// Records that a source was read in full.
    ///
    /// # Errors
    /// The store failed.
    fn set_checked(&self, source: Source, at: i64) -> Done<()>;
}

/// Restarts a container through the allowlisted container action.
pub trait Restarter {
    /// Restarts the container with this id.
    ///
    /// # Errors
    /// Why it was not restarted, as the owner may read it.
    fn restart(&self, target: &str) -> Result<(), String>;
}

/// Pushes to the owner's phone.
pub trait Pusher {
    /// Pushes a short line.
    ///
    /// # Errors
    /// Why it was not pushed.
    fn push(&self, text: &str) -> Result<(), String>;
}

/// Endora's Activity: one line per thing the agenda did or decided.
pub trait Activity {
    /// Records a line.
    fn record(&self, at: i64, line: &str);
}
