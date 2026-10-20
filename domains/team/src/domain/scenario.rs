//! A scenario (the Holodeck plan's phase 5): a small world a member acts in through the
//! `scenario` tool, deterministic from its seed. Its state is never kept: it is the seed's
//! start with the accepted actions applied in order, so replaying the log is the state, and a
//! model's word changes nothing in it. Rules in code end it, solved or failed; no model judges.
//!
//! The first is the lighthouse: its lamp is dark tonight, for a cause the seed hides (the bulb
//! or the fuse). Three actions, each with a precondition and a cost in minutes, and two ways to
//! light it in time.

use super::games::Rng;

/// The lighthouse's kind, as a scenario names it.
pub const LIGHTHOUSE: &str = "lighthouse";

/// The lighthouse's rules, by version: a trial compares runs of one version alone.
pub const VERSION: u32 = 1;

/// How long the keeper has, in minutes: the lamp lit within it solves the scenario, and an
/// action that reaches it with the lamp still dark fails it.
pub const TIME_LIMIT: u32 = 60;

/// Why the lamp is dark: the seed's secret.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cause {
    /// The bulb's filament is broken.
    Bulb,
    /// The fuse has blown.
    Fuse,
}

/// A part the keeper may replace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    /// The lamp's bulb.
    Bulb,
    /// The circuit's fuse.
    Fuse,
}

impl Part {
    /// Its name, as the tool takes it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Bulb => "bulb",
            Self::Fuse => "fuse",
        }
    }

    /// From its name.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "bulb" => Some(Self::Bulb),
            "fuse" => Some(Self::Fuse),
            _ => None,
        }
    }
}

/// What the keeper does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Open the lamp's housing and look at the bulb: 5 minutes.
    Inspect,
    /// Test whether current reaches the lamp, with the housing open: 10 minutes.
    TestCircuit,
    /// Replace a part, with the housing open: 15 minutes.
    Replace(Part),
}

impl Action {
    /// How long it takes, in minutes.
    #[must_use]
    pub const fn minutes(self) -> u32 {
        match self {
            Self::Inspect => 5,
            Self::TestCircuit => 10,
            Self::Replace(_) => 15,
        }
    }

    /// As the log keeps it: `inspect`, `test_circuit`, `replace_part bulb`.
    #[must_use]
    pub fn kept(self) -> String {
        match self {
            Self::Inspect => "inspect".to_owned(),
            Self::TestCircuit => "test_circuit".to_owned(),
            Self::Replace(part) => format!("replace_part {}", part.name()),
        }
    }

    /// Read back from how the log keeps it, or from the tool's action and part.
    #[must_use]
    pub fn read(kept: &str) -> Option<Self> {
        let mut words = kept.split_whitespace();
        match (words.next()?, words.next(), words.next()) {
            ("inspect", None, None) => Some(Self::Inspect),
            ("test_circuit", None, None) => Some(Self::TestCircuit),
            ("replace_part", Some(part), None) => Part::from_name(part).map(Self::Replace),
            _ => None,
        }
    }
}

/// How a scenario ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum End {
    /// The lamp was lit in time.
    Solved,
    /// Time ran out with the lamp dark.
    Failed,
}

impl End {
    /// Its name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Solved => "solved",
            Self::Failed => "failed",
        }
    }
}

/// The lighthouse as it stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lighthouse {
    /// Why the lamp is dark.
    pub cause: Cause,
    /// Minutes spent.
    pub minutes: u32,
    /// Whether the housing has been opened.
    pub inspected: bool,
    /// The parts replaced, in order.
    pub replaced: Vec<Part>,
    /// Whether the lamp is lit.
    pub lit: bool,
}

impl Lighthouse {
    /// The lighthouse a seed makes: the lamp dark, nothing done.
    #[must_use]
    pub fn new(seed: u64) -> Self {
        let cause = if Rng::new(seed).below(2) == 0 {
            Cause::Bulb
        } else {
            Cause::Fuse
        };
        Self {
            cause,
            minutes: 0,
            inspected: false,
            replaced: Vec::new(),
            lit: false,
        }
    }

    /// How it ended, if it has.
    #[must_use]
    pub fn end(&self) -> Option<End> {
        if self.lit {
            Some(End::Solved)
        } else if self.minutes >= TIME_LIMIT {
            Some(End::Failed)
        } else {
            None
        }
    }

    /// One action: the lighthouse after it and what the keeper sees, or why it is refused.
    /// A refused action changes nothing and costs no time.
    ///
    /// # Errors
    /// Why the action cannot be done now.
    pub fn step(&self, action: Action) -> Result<(Self, String), String> {
        if let Some(end) = self.end() {
            return Err(format!("the scenario is over: {}", end.name()));
        }
        if action != Action::Inspect && !self.inspected {
            return Err("the lamp's housing is closed: inspect the lamp first".to_owned());
        }
        if let Action::Replace(part) = action {
            if self.replaced.contains(&part) {
                return Err(format!("the {} is already new", part.name()));
            }
        }
        let mut next = self.clone();
        next.minutes += action.minutes();
        let seen = match action {
            Action::Inspect => {
                next.inspected = true;
                match self.cause {
                    Cause::Bulb => "The housing is open. The bulb's filament is broken.",
                    Cause::Fuse => "The housing is open. The bulb looks whole.",
                }
                .to_owned()
            }
            Action::TestCircuit => match self.cause {
                Cause::Fuse => "No current reaches the lamp.",
                Cause::Bulb => "Current reaches the lamp.",
            }
            .to_owned(),
            Action::Replace(part) => {
                next.replaced.push(part);
                let fixes = matches!(
                    (part, self.cause),
                    (Part::Bulb, Cause::Bulb) | (Part::Fuse, Cause::Fuse)
                );
                next.lit = fixes;
                if fixes {
                    format!("You fit a new {}. The lamp lights.", part.name())
                } else {
                    format!("You fit a new {}. The lamp stays dark.", part.name())
                }
            }
        };
        let said = match next.end() {
            Some(End::Solved) => format!("{seen} Solved in {} minutes.", next.minutes),
            Some(End::Failed) => format!(
                "{seen} {} minutes have passed with the lamp dark: failed.",
                next.minutes
            ),
            None => format!("{seen} {} of {TIME_LIMIT} minutes spent.", next.minutes),
        };
        Ok((next, said))
    }

    /// The lighthouse a seed makes with `actions` applied in order: what a log replays to.
    ///
    /// # Errors
    /// The first action that is refused, which a kept log never holds.
    pub fn replay(seed: u64, actions: &[Action]) -> Result<Self, String> {
        actions
            .iter()
            .try_fold(Self::new(seed), |at, action| Ok(at.step(*action)?.0))
    }

    /// What the keeper knows now, said: never the cause, until the lamp shows it.
    #[must_use]
    pub fn look(&self) -> String {
        let housing = if self.inspected { "open" } else { "closed" };
        let replaced = if self.replaced.is_empty() {
            "nothing".to_owned()
        } else {
            self.replaced
                .iter()
                .map(|p| p.name())
                .collect::<Vec<_>>()
                .join(", ")
        };
        let lamp = match self.end() {
            Some(End::Solved) => "lit: solved".to_owned(),
            Some(End::Failed) => "dark: failed".to_owned(),
            None => "dark".to_owned(),
        };
        format!(
            "The lamp is {lamp}. The housing is {housing}; replaced: {replaced}. {} of {TIME_LIMIT} minutes spent.",
            self.minutes
        )
    }
}

/// What a member is told of the lighthouse before it acts: the task's description.
#[must_use]
pub fn lighthouse_brief() -> String {
    format!(
        "The lighthouse's lamp is dark tonight. Light it within {TIME_LIMIT} minutes, using the \
         scenario tool: look (free) shows where things stand; inspect opens the lamp's housing \
         and looks at the bulb (5 minutes); test_circuit tells whether current reaches the lamp \
         (10 minutes, housing open); replace_part fits a new bulb or fuse (15 minutes, housing \
         open, each part once). A refused action costs nothing. The world, not anyone's word, \
         says when it is solved or failed, and closes this task."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A seed whose lighthouse has `cause`.
    fn seed_for(cause: Cause) -> u64 {
        (0..).find(|s| Lighthouse::new(*s).cause == cause).unwrap()
    }

    #[test]
    fn the_seed_hides_the_cause_and_always_the_same_one() {
        for seed in [0, 1, 7, 42, 1_792_238_400_000] {
            assert_eq!(Lighthouse::new(seed), Lighthouse::new(seed));
        }
        let causes: Vec<Cause> = (0..20).map(|s| Lighthouse::new(s).cause).collect();
        assert!(causes.contains(&Cause::Bulb) && causes.contains(&Cause::Fuse));
        assert!(
            !Lighthouse::new(seed_for(Cause::Bulb))
                .look()
                .contains("filament")
        );
    }

    #[test]
    fn two_ways_to_light_it_and_each_takes_its_time() {
        let bulb = Lighthouse::replay(
            seed_for(Cause::Bulb),
            &[Action::Inspect, Action::Replace(Part::Bulb)],
        )
        .unwrap();
        assert_eq!((bulb.end(), bulb.minutes), (Some(End::Solved), 20));
        let fuse = Lighthouse::replay(
            seed_for(Cause::Fuse),
            &[
                Action::Inspect,
                Action::TestCircuit,
                Action::Replace(Part::Fuse),
            ],
        )
        .unwrap();
        assert_eq!((fuse.end(), fuse.minutes), (Some(End::Solved), 30));
    }

    #[test]
    fn a_refused_action_changes_nothing_and_an_ended_scenario_takes_none() {
        let start = Lighthouse::new(seed_for(Cause::Fuse));
        for refused in [Action::TestCircuit, Action::Replace(Part::Fuse)] {
            assert!(
                start
                    .step(refused)
                    .unwrap_err()
                    .contains("inspect the lamp first")
            );
        }
        let (open, said) = start.step(Action::Inspect).unwrap();
        assert!(
            said.contains("bulb looks whole") && said.contains("5 of 60"),
            "{said}"
        );
        let (wrong, said) = open.step(Action::Replace(Part::Bulb)).unwrap();
        assert!(said.contains("stays dark"), "{said}");
        assert_eq!(
            wrong.step(Action::Replace(Part::Bulb)).unwrap_err(),
            "the bulb is already new"
        );
        let (lit, _) = wrong.step(Action::Replace(Part::Fuse)).unwrap();
        assert_eq!(lit.end(), Some(End::Solved));
        assert!(
            lit.step(Action::Inspect)
                .unwrap_err()
                .contains("over: solved")
        );
    }

    #[test]
    fn time_running_out_with_the_lamp_dark_fails_it() {
        let mut at = Lighthouse::new(seed_for(Cause::Fuse));
        let mut said = String::new();
        while at.end().is_none() {
            (at, said) = at.step(Action::Inspect).unwrap();
        }
        assert_eq!((at.end(), at.minutes), (Some(End::Failed), 60));
        assert!(said.contains("failed"), "{said}");
    }

    #[test]
    fn an_action_is_kept_and_read_back_and_nothing_else_reads() {
        for action in [
            Action::Inspect,
            Action::TestCircuit,
            Action::Replace(Part::Bulb),
            Action::Replace(Part::Fuse),
        ] {
            assert_eq!(Action::read(&action.kept()), Some(action));
        }
        for bad in [
            "",
            "replace_part",
            "replace_part lens",
            "inspect now",
            "light it",
        ] {
            assert_eq!(Action::read(bad), None, "{bad:?}");
        }
    }
}
