//! The agenda's rules, pure: kinds of signal and what each may become, the levels, the
//! restart caps, and how the containers' states become signals.

mod kinds;
mod restarts;
mod sources;

pub use kinds::{Family, Kind, Level, Response, Source};
pub use restarts::{RestartVerdict, may_restart};
pub use sources::{Container, QueuedDownload, container_signals, download_signals, event_signals};

/// One minute, in milliseconds.
pub const MINUTE_MS: i64 = 60 * 1000;
/// One hour, in milliseconds.
pub const HOUR_MS: i64 = 60 * MINUTE_MS;
/// One day, in milliseconds.
pub const DAY_MS: i64 = 24 * HOUR_MS;

/// How many things it watches with a deadline at once; past that, a new one is told at its
/// deadline's level straight away.
pub const MOST_WATCHED: usize = 3;
/// How many urgent pushes in a day, at most.
pub const MOST_PUSHES_A_DAY: usize = 5;
/// How long after a restart it waits to see the container running before telling the owner
/// the restart did not take.
pub const RESTART_TAKES_MS: i64 = 5 * MINUTE_MS;
/// How long an FYI about something that happened (mail) stays before it closes itself.
pub const EVENT_FYI_LASTS_MS: i64 = DAY_MS;

/// Something Endora read that may deserve an item. Built only by code from checked facts;
/// the text in it is shown, never obeyed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signal {
    /// What kind of thing it is.
    pub kind: Kind,
    /// Which one, within the kind: a container's name, a message's key, a day and title.
    /// With the kind, the item's identity.
    pub subject: String,
    /// What a taught rule matches: a container's name, a sender's address, a title.
    pub about: String,
    /// The name it is shown by.
    pub name: String,
    /// The fact behind it, worded by code: an exit code, the mail's checked line, a day.
    pub detail: String,
    /// For a container, its id: what a restart acts on.
    pub target: Option<String>,
    /// For mail: it may not be what it says, so it is never more than FYI.
    pub cautious: bool,
}

impl Signal {
    /// The item's identity: the same thing seen again is the same item.
    #[must_use]
    pub fn key(&self) -> String {
        format!("{}:{}", self.kind.name(), self.subject)
    }

    /// What the owner reads: fixed wording over the checked facts.
    #[must_use]
    pub fn sentence(&self) -> String {
        let (name, detail) = (&self.name, &self.detail);
        match self.kind {
            Kind::ContainerCrashed => format!("{name} crashed ({detail})"),
            Kind::ContainerUnhealthy => format!("{name} is unhealthy"),
            Kind::ContainerLooping => format!("{name} keeps restarting"),
            Kind::ContainerStopped => {
                format!("{name} stopped ({detail}); left alone, as that looks deliberate")
            }
            Kind::DownloadStuck => format!("A download is stuck in {detail}: {name}"),
            Kind::MailAccountSecurity
            | Kind::MailMoneyDue
            | Kind::MailWaitingOnYou
            | Kind::MailFromAPerson
            | Kind::MailAppointment
            | Kind::MailOrderProblem
            | Kind::MailAboutYourApp => detail.clone(),
            Kind::Event => format!("{detail}: {name}"),
        }
    }
}

/// Whether a taught rule covers a signal: the same family, about the same thing.
#[must_use]
pub fn covered(family: Family, about: &str, signal: &Signal) -> bool {
    signal.kind.family() == family && about.trim().eq_ignore_ascii_case(signal.about.trim())
}

#[cfg(test)]
mod tests;
