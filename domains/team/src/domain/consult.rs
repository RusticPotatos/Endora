//! Retained legacy task-consultation records (ADR 0138), with no execution policy.

use super::work::{MemberId, Millis, RunId, TaskId};

/// Where a consult is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Its member was woken and has not answered.
    Queued,
    /// Its member answered.
    Answered,
    /// Its member was not woken, or did not answer: the record answered.
    FromRecord,
    /// Explicitly closed at retirement, without inventing an answer.
    Cancelled,
}

impl Status {
    /// Its stable name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Answered => "answered",
            Self::FromRecord => "from_record",
            Self::Cancelled => "cancelled",
        }
    }

    /// The status with this [`name`](Self::name).
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        [
            Self::Queued,
            Self::Answered,
            Self::FromRecord,
            Self::Cancelled,
        ]
        .into_iter()
        .find(|s| s.name() == name)
    }
}

/// One consult, as kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Consult {
    /// Its row; 0 before it is kept.
    pub id: i64,
    /// Who asked.
    pub asker: MemberId,
    /// The turn it asked in.
    pub asker_run: RunId,
    /// Who was asked.
    pub expert: MemberId,
    /// The task the question is about: what the expert's handover notes and last words are read
    /// on.
    pub topic: Option<TaskId>,
    /// The task whose thread the answer goes in.
    pub thread: TaskId,
    /// The question, cleaned.
    pub question: String,
    /// Where it is.
    pub status: Status,
    /// The turn that answered it, once the expert's turn began.
    pub answer_run: Option<RunId>,
    /// Why it was answered from the record, if it was.
    pub why: String,
    /// Whether the asker has been woken to read the answer.
    pub asker_woken: bool,
    /// When it was asked.
    pub at: Millis,
}

#[cfg(test)]
mod tests {
    use super::Status;

    #[test]
    fn retained_statuses_round_trip_including_explicit_cancellation() {
        for status in [
            Status::Queued,
            Status::Answered,
            Status::FromRecord,
            Status::Cancelled,
        ] {
            assert_eq!(Status::from_name(status.name()), Some(status));
        }
        assert_eq!(Status::from_name("unknown"), None);
    }
}
