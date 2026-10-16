//! The kinds of signal, what each may become, and the levels it is told at.

use super::{DAY_MS, HOUR_MS, MINUTE_MS};

/// Where a signal came from. A source reports everything it sees in one observation, or
/// nothing when it could not be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Source {
    /// The containers, through the home-lab observer.
    Docker,
    /// Sonarr's download queue.
    Sonarr,
    /// Radarr's download queue.
    Radarr,
    /// The mail the reader has sorted.
    Mail,
    /// The calendar, today and tomorrow.
    Calendar,
    /// The house's monitoring: Prometheus's firing alerts and Gatus's failing checks.
    Monitoring,
    /// Endora itself: its nightly copy.
    Endora,
}

impl Source {
    /// Every source.
    pub const ALL: [Self; 7] = [
        Self::Docker,
        Self::Sonarr,
        Self::Radarr,
        Self::Mail,
        Self::Calendar,
        Self::Monitoring,
        Self::Endora,
    ];

    /// Its stored name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Docker => "docker",
            Self::Sonarr => "sonarr",
            Self::Radarr => "radarr",
            Self::Mail => "mail",
            Self::Calendar => "calendar",
            Self::Monitoring => "monitoring",
            Self::Endora => "endora",
        }
    }

    /// From its stored name.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.name() == name)
    }

    /// Whether what it reports is a state that clears (a container running again), so an item
    /// closes when a full observation no longer has it. Mail reports things that happened; they
    /// do not clear by being absent.
    #[must_use]
    pub const fn reports_states(self) -> bool {
        !matches!(self, Self::Mail)
    }
}

/// What a taught rule is about: Stop on one container's item leaves that container alone,
/// whatever happens to it next.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Family {
    /// A container.
    Container,
    /// A download.
    Download,
    /// Mail from a sender.
    Mail,
    /// A calendar entry.
    Calendar,
    /// One of the house's alerts or checks.
    Alert,
    /// Endora's nightly copy.
    NightlyCopy,
}

impl Family {
    /// Every family.
    pub const ALL: [Self; 6] = [
        Self::Container,
        Self::Download,
        Self::Mail,
        Self::Calendar,
        Self::Alert,
        Self::NightlyCopy,
    ];

    /// Its stored name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Container => "container",
            Self::Download => "download",
            Self::Mail => "mail",
            Self::Calendar => "calendar",
            Self::Alert => "alert",
            Self::NightlyCopy => "nightly_copy",
        }
    }

    /// From its stored name.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|f| f.name() == name)
    }

    /// The rule, as the owner reads it in Settings.
    #[must_use]
    pub fn rule_said(self, about: &str) -> String {
        match self {
            Self::Container => format!("Leave the container {about} alone"),
            Self::Download => format!("Don't tell me about the download {about}"),
            Self::Mail => format!("Don't tell me about mail from {about}"),
            Self::Calendar => format!("Don't tell me about \"{about}\" on the calendar"),
            Self::Alert => format!("Don't tell me about {about}"),
            Self::NightlyCopy => "Don't tell me about Endora's nightly copy".to_owned(),
        }
    }
}

/// How an item is told. Handled and noted things are a line in Activity; FYI is on the Now
/// line and in the next brief; needs you is pinned in chat; urgent is pinned and pushed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Level {
    /// A line in Activity.
    Handled,
    /// On the Now line and in the next brief.
    Fyi,
    /// Pinned at the top of the chat until answered.
    NeedsYou,
    /// Pinned, and pushed once.
    Urgent,
}

impl Level {
    /// Its stored name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Handled => "handled",
            Self::Fyi => "fyi",
            Self::NeedsYou => "needs_you",
            Self::Urgent => "urgent",
        }
    }

    /// From its stored name.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        [Self::Handled, Self::Fyi, Self::NeedsYou, Self::Urgent]
            .into_iter()
            .find(|l| l.name() == name)
    }

    /// As the owner reads it.
    #[must_use]
    pub const fn said(self) -> &'static str {
        match self {
            Self::Handled => "Handled",
            Self::Fyi => "FYI",
            Self::NeedsYou => "Needs you",
            Self::Urgent => "Urgent",
        }
    }
}

/// What the rules decide for a new signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Response {
    /// Restart the container.
    Handle,
    /// Watch it with a deadline.
    LookInto,
    /// A line in Activity, nothing more.
    Note,
    /// Tell at FYI.
    Fyi,
    /// Tell, pinned.
    NeedsYou,
    /// Tell, pinned and pushed.
    Urgent,
}

impl Response {
    /// Every response, in the order Settings offers them.
    pub const ALL: [Self; 6] = [
        Self::Handle,
        Self::LookInto,
        Self::Note,
        Self::Fyi,
        Self::NeedsYou,
        Self::Urgent,
    ];

    /// Its stored name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Handle => "handle",
            Self::LookInto => "look_into",
            Self::Note => "note",
            Self::Fyi => "fyi",
            Self::NeedsYou => "needs_you",
            Self::Urgent => "urgent",
        }
    }

    /// From its stored name.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|r| r.name() == name)
    }

    /// As the owner reads it in Settings.
    #[must_use]
    pub const fn said(self) -> &'static str {
        match self {
            Self::Handle => "Restart it",
            Self::LookInto => "Watch it",
            Self::Note => "Just note it",
            Self::Fyi => "FYI",
            Self::NeedsYou => "Needs you",
            Self::Urgent => "Urgent",
        }
    }

    /// The level an item starts at under this response.
    #[must_use]
    pub const fn level(self) -> Level {
        match self {
            Self::Handle | Self::Note => Level::Handled,
            Self::LookInto | Self::Fyi => Level::Fyi,
            Self::NeedsYou => Level::NeedsYou,
            Self::Urgent => Level::Urgent,
        }
    }
}

/// A kind of signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    /// A container Endora saw running exited with an error.
    ContainerCrashed,
    /// A running container's health check is failing.
    ContainerUnhealthy,
    /// A container is stuck restarting.
    ContainerLooping,
    /// A container Endora saw running stopped cleanly: most likely on purpose.
    ContainerStopped,
    /// A download is stuck in a queue.
    DownloadStuck,
    /// Mail about the owner's account security.
    MailAccountSecurity,
    /// Mail with money due.
    MailMoneyDue,
    /// Someone is waiting on the owner's reply.
    MailWaitingOnYou,
    /// Someone the owner writes to wrote.
    MailFromAPerson,
    /// Mail about an appointment.
    MailAppointment,
    /// A delivery or order went wrong.
    MailOrderProblem,
    /// Mail about one of the owner's apps.
    MailAboutYourApp,
    /// Something on the calendar today or tomorrow.
    Event,
    /// A filesystem is full.
    DiskFull,
    /// A filesystem is almost full.
    DiskAlmostFull,
    /// A backup or a database dump has not succeeded lately.
    BackupFailing,
    /// A drive is failing its health check or losing sectors.
    DriveFailing,
    /// Any other critical alert.
    AlertCritical,
    /// Any other warning.
    AlertWarning,
    /// One of the house's uptime checks is failing.
    CheckFailing,
    /// Endora could not save its nightly copy.
    NightlyCopyFailed,
}

impl Kind {
    /// Every kind, in the order Settings lists them.
    pub const ALL: [Self; 21] = [
        Self::ContainerCrashed,
        Self::ContainerUnhealthy,
        Self::ContainerLooping,
        Self::ContainerStopped,
        Self::DiskFull,
        Self::DiskAlmostFull,
        Self::BackupFailing,
        Self::DriveFailing,
        Self::CheckFailing,
        Self::AlertCritical,
        Self::AlertWarning,
        Self::NightlyCopyFailed,
        Self::DownloadStuck,
        Self::MailAccountSecurity,
        Self::MailMoneyDue,
        Self::MailWaitingOnYou,
        Self::MailFromAPerson,
        Self::MailAppointment,
        Self::MailOrderProblem,
        Self::MailAboutYourApp,
        Self::Event,
    ];

    /// Its stored name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::ContainerCrashed => "container_crashed",
            Self::ContainerUnhealthy => "container_unhealthy",
            Self::ContainerLooping => "container_looping",
            Self::ContainerStopped => "container_stopped",
            Self::DownloadStuck => "download_stuck",
            Self::MailAccountSecurity => "mail_account_security",
            Self::MailMoneyDue => "mail_money_due",
            Self::MailWaitingOnYou => "mail_waiting_on_you",
            Self::MailFromAPerson => "mail_from_a_person",
            Self::MailAppointment => "mail_appointment",
            Self::MailOrderProblem => "mail_order_problem",
            Self::MailAboutYourApp => "mail_about_your_app",
            Self::Event => "event",
            Self::DiskFull => "disk_full",
            Self::DiskAlmostFull => "disk_almost_full",
            Self::BackupFailing => "backup_failing",
            Self::DriveFailing => "drive_failing",
            Self::AlertCritical => "alert_critical",
            Self::AlertWarning => "alert_warning",
            Self::CheckFailing => "check_failing",
            Self::NightlyCopyFailed => "nightly_copy_failed",
        }
    }

    /// From its stored name.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.name() == name)
    }

    /// As the owner reads it in Settings.
    #[must_use]
    pub const fn said(self) -> &'static str {
        match self {
            Self::ContainerCrashed => "A container crashed",
            Self::ContainerUnhealthy => "A container is unhealthy",
            Self::ContainerLooping => "A container keeps restarting",
            Self::ContainerStopped => "A container stopped cleanly",
            Self::DownloadStuck => "A download is stuck",
            Self::MailAccountSecurity => "Mail about your account security",
            Self::MailMoneyDue => "Mail with money due",
            Self::MailWaitingOnYou => "Someone is waiting on your reply",
            Self::MailFromAPerson => "Someone you write to wrote",
            Self::MailAppointment => "Mail about an appointment",
            Self::MailOrderProblem => "A delivery or order went wrong",
            Self::MailAboutYourApp => "Mail about your app",
            Self::Event => "On the calendar today or tomorrow",
            Self::DiskFull => "A disk is full",
            Self::DiskAlmostFull => "A disk is almost full",
            Self::BackupFailing => "A backup hasn't succeeded lately",
            Self::DriveFailing => "A drive is failing",
            Self::AlertCritical => "Any other critical alert",
            Self::AlertWarning => "Any other warning",
            Self::CheckFailing => "An uptime check is failing",
            Self::NightlyCopyFailed => "Endora's nightly copy failed",
        }
    }

    /// What a Stop on its item teaches about.
    #[must_use]
    pub const fn family(self) -> Family {
        match self {
            Self::ContainerCrashed
            | Self::ContainerUnhealthy
            | Self::ContainerLooping
            | Self::ContainerStopped => Family::Container,
            Self::DownloadStuck => Family::Download,
            Self::MailAccountSecurity
            | Self::MailMoneyDue
            | Self::MailWaitingOnYou
            | Self::MailFromAPerson
            | Self::MailAppointment
            | Self::MailOrderProblem
            | Self::MailAboutYourApp => Family::Mail,
            Self::Event => Family::Calendar,
            Self::DiskFull
            | Self::DiskAlmostFull
            | Self::BackupFailing
            | Self::DriveFailing
            | Self::AlertCritical
            | Self::AlertWarning
            | Self::CheckFailing => Family::Alert,
            Self::NightlyCopyFailed => Family::NightlyCopy,
        }
    }

    /// What the rules do with it unless the owner re-levelled it (Q2–Q4, ADR 0103).
    #[must_use]
    pub const fn default_response(self) -> Response {
        match self {
            Self::ContainerCrashed | Self::ContainerUnhealthy => Response::Handle,
            Self::ContainerLooping | Self::DownloadStuck | Self::CheckFailing => Response::LookInto,
            Self::ContainerStopped => Response::Note,
            Self::MailAccountSecurity | Self::DiskFull => Response::Urgent,
            Self::MailMoneyDue
            | Self::MailWaitingOnYou
            | Self::DiskAlmostFull
            | Self::BackupFailing
            | Self::DriveFailing
            | Self::AlertCritical
            | Self::NightlyCopyFailed => Response::NeedsYou,
            Self::MailFromAPerson
            | Self::MailAppointment
            | Self::MailOrderProblem
            | Self::MailAboutYourApp
            | Self::Event
            | Self::AlertWarning => Response::Fyi,
        }
    }

    /// Whether the owner may set it to this response. Only a container that crashed or went
    /// unhealthy can be restarted; only what can clear on its own can be watched.
    #[must_use]
    pub const fn allows(self, response: Response) -> bool {
        match response {
            Response::Handle => matches!(self, Self::ContainerCrashed | Self::ContainerUnhealthy),
            Response::LookInto => matches!(
                self,
                Self::ContainerLooping
                    | Self::DownloadStuck
                    | Self::CheckFailing
                    | Self::AlertCritical
                    | Self::AlertWarning
            ),
            Response::Note | Response::Fyi | Response::NeedsYou | Response::Urgent => true,
        }
    }

    /// For something watched: how long it watches, and the level it is told at if it has not
    /// cleared by then.
    #[must_use]
    pub const fn watch(self) -> (i64, Level) {
        match self {
            Self::ContainerLooping => (30 * MINUTE_MS, Level::Urgent),
            Self::CheckFailing => (15 * MINUTE_MS, Level::NeedsYou),
            Self::AlertCritical | Self::AlertWarning => (HOUR_MS, Level::NeedsYou),
            _ => (DAY_MS, Level::NeedsYou),
        }
    }

    /// For something told: whether it is told louder if it has not cleared after a while. A
    /// backup still failing a day after it was told is failing two days running (Q4).
    #[must_use]
    pub const fn escalates(self) -> Option<(i64, Level)> {
        match self {
            Self::BackupFailing => Some((DAY_MS, Level::Urgent)),
            _ => None,
        }
    }

    /// Whether it came from mail: it can only tell, and an urgent push about it names nothing.
    #[must_use]
    pub const fn from_mail(self) -> bool {
        matches!(self.family(), Family::Mail)
    }
}
