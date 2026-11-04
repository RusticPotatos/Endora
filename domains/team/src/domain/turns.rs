//! Where a turn runs (ADR 0131, amended by 0142): a member is its model, so every turn of its
//! runs there, whatever woke it. A turn on Claude or DeepSeek is paid, and waits when the money
//! is gone; a turn on the house's models costs nothing, waits for the Mac when it is on the Mac,
//! and is capped by the day. The language model proposes; this authorizes.

use super::brain::Brain;

/// What a turn is, by the brain it runs on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TurnKind {
    /// On the house's models: it costs nothing.
    Work,
    /// On a model the team pays for, from its pool.
    Judgment,
}

impl TurnKind {
    /// The kind of a turn on `brain`.
    #[must_use]
    pub const fn on(brain: Brain) -> Self {
        if brain.cloud() {
            return Self::Judgment;
        }
        Self::Work
    }

    /// What the owner reads.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Work => "work",
            Self::Judgment => "judgment",
        }
    }
}

/// Why a turn waits instead of starting, as the owner reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Wait {
    /// It is judgment, and today's budget is spent: the pool's day, the member's share, or its
    /// project's month.
    Budget,
    /// It needs the Mac, and the Mac cannot take it now, for this reason (ADR 0130).
    Mac(String),
    /// It is work on the house's own models, which cost nothing, and the member's turns on them
    /// today have shown nothing too many times in a row (ADR 0133, amended 2026-11-01: waste,
    /// not a count): it waits until the day turns.
    DayCap,
    /// A restart is coming and a drain holds every new turn back (ADR 0134): it starts once
    /// HQ is up again.
    Restart,
}

/// The prefixes a waiting turn's words begin with.
const WAITING: [&str; 4] = [
    "waiting for the Mac: ",
    "waiting for budget: ",
    "waiting for tomorrow: ",
    "waiting for a restart: ",
];

impl Wait {
    /// The turn's `why` while it waits.
    #[must_use]
    pub fn words(&self) -> String {
        match self {
            Self::Budget => "waiting for budget: the budget for it is spent".to_owned(),
            Self::Mac(why) => super::mac::waiting_words(why),
            Self::DayCap => format!(
                "{}{} turns in a row on the house's own models showed nothing",
                WAITING[2],
                super::guards::THRESHOLDS.free_turns_wasted
            ),
            Self::Restart => format!("{}nothing new starts until it has been", WAITING[3]),
        }
    }
}

/// Whether `said` is a turn's waiting words (see [`Wait::words`]): the turn's reason, which the
/// engine clears when the turn starts.
#[must_use]
pub fn is_waiting_words(said: &str) -> bool {
    WAITING.iter().any(|prefix| said.starts_with(prefix))
}

/// Whether `said` is a turn waiting for budget.
#[must_use]
pub fn is_waiting_for_budget(said: &str) -> bool {
    said.starts_with(WAITING[1])
}

/// Whether `said` is a turn waiting for the day to turn, a member having started its day's turns
/// on the house's own models (ADR 0133).
#[must_use]
pub fn is_waiting_for_cap(said: &str) -> bool {
    said.starts_with(WAITING[2])
}

/// What decides where a turn runs, besides its brain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Facts {
    /// Why the Mac cannot take a turn now (see [`super::mac::wait_for`]); `None` when it can.
    pub mac_waits: Option<String>,
    /// Whether the budget lets a paid turn start now.
    pub can_afford: bool,
    /// How many of the member's turns today on the house's own models, newest back, showed
    /// nothing (ADR 0133, amended 2026-11-01): a turn that did something ends the run.
    pub free_turns_wasted: usize,
}

/// Where a turn goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Placed {
    /// It starts on this brain.
    On(Brain),
    /// It waits, and says why. It is never given another brain.
    Waits(Wait),
}

/// Where a turn on `brain` runs: there, or it waits. A paid turn waits for the money; a turn on
/// the house's own models, after the member's turns today showed nothing too many times in a
/// row, waits for the day to turn (ADR 0133, amended 2026-11-01), and one on the Mac waits for
/// the Mac.
#[must_use]
pub fn place(brain: Brain, facts: &Facts) -> Placed {
    if brain.cloud() {
        if facts.can_afford {
            return Placed::On(brain);
        }
        return Placed::Waits(Wait::Budget);
    }
    if facts.free_turns_wasted >= super::guards::THRESHOLDS.free_turns_wasted {
        return Placed::Waits(Wait::DayCap);
    }
    match (brain, &facts.mac_waits) {
        (Brain::Mac, Some(why)) => Placed::Waits(Wait::Mac(why.clone())),
        _ => Placed::On(brain),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(mac_waits: Option<&str>) -> Facts {
        Facts {
            mac_waits: mac_waits.map(str::to_owned),
            can_afford: true,
            free_turns_wasted: 0,
        }
    }

    #[test]
    fn a_paid_turn_runs_on_its_model_and_waits_when_the_money_is_gone() {
        assert_eq!(
            place(Brain::Sonnet, &facts(None)),
            Placed::On(Brain::Sonnet)
        );
        let broke = Facts {
            can_afford: false,
            free_turns_wasted: 500,
            ..facts(None)
        };
        assert_eq!(place(Brain::Sonnet, &broke), Placed::Waits(Wait::Budget));
        assert_eq!(TurnKind::on(Brain::DeepSeek), TurnKind::Judgment);
    }

    #[test]
    fn a_turn_on_the_mac_waits_for_the_mac_and_is_never_given_the_nas_model() {
        assert_eq!(
            place(Brain::Mac, &facts(Some("owner active"))),
            Placed::Waits(Wait::Mac("owner active".to_owned()))
        );
        assert_eq!(place(Brain::Mac, &facts(None)), Placed::On(Brain::Mac));
        assert_eq!(
            place(Brain::Local, &facts(Some("owner active"))),
            Placed::On(Brain::Local)
        );
        let broke = Facts {
            can_afford: false,
            ..facts(None)
        };
        assert_eq!(place(Brain::Local, &broke), Placed::On(Brain::Local));
        assert_eq!(TurnKind::on(Brain::Mac), TurnKind::Work);
    }

    /// D15 of round 3 (31 Oct): Bo, on the house's own model, which costs nothing, stopped at
    /// "60 turns a day" whatever his turns achieved. A count is not what should stop a free
    /// model; waste is. Turns that do work never wait, however many; the tenth turn in a row
    /// that showed nothing (or failed) waits until the day turns, the Mac's turns alike.
    #[test]
    fn a_free_model_waits_for_tomorrow_on_waste_not_on_a_count() {
        let wasted = |n| Facts {
            free_turns_wasted: n,
            ..facts(Some("owner active"))
        };
        assert_eq!(place(Brain::Local, &wasted(0)), Placed::On(Brain::Local));
        assert_eq!(place(Brain::Local, &wasted(9)), Placed::On(Brain::Local));
        for brain in [Brain::Local, Brain::Mac] {
            assert_eq!(place(brain, &wasted(10)), Placed::Waits(Wait::DayCap));
        }
        assert_eq!(place(Brain::Haiku, &wasted(500)), Placed::On(Brain::Haiku));
        assert_eq!(
            Wait::DayCap.words(),
            "waiting for tomorrow: 10 turns in a row on the house's own models showed nothing"
        );
    }

    #[test]
    fn waiting_words_are_recognised_so_they_can_be_cleared() {
        for wait in [
            Wait::Budget,
            Wait::Mac("on battery".to_owned()),
            Wait::DayCap,
        ] {
            assert!(is_waiting_words(&wait.words()), "{wait:?}");
        }
        assert!(is_waiting_for_budget(&Wait::Budget.words()));
        assert!(!is_waiting_for_budget(
            &Wait::Mac("on battery".to_owned()).words()
        ));
        assert!(is_waiting_for_cap(&Wait::DayCap.words()));
        assert!(!is_waiting_for_cap(&Wait::Budget.words()));
        assert!(!is_waiting_for_budget(&Wait::DayCap.words()));
        assert!(
            Wait::DayCap.words().starts_with("waiting for "),
            "the roster line shows every wait that begins so"
        );
        assert!(!is_waiting_words(
            "backing off: its last turns moved nothing"
        ));
    }

    /// A turn held back by a drain (ADR 0134) says so in the words the roster shows, and is a
    /// wait like the rest: recognised, so the engine clears it when the turn starts.
    #[test]
    fn a_turn_held_for_a_restart_says_so() {
        let words = Wait::Restart.words();
        assert_eq!(
            words,
            "waiting for a restart: nothing new starts until it has been"
        );
        assert!(is_waiting_words(&words));
        assert!(!is_waiting_for_budget(&words) && !is_waiting_for_cap(&words));
    }
}
