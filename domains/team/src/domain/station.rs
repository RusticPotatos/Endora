//! Silent Station (the Holodeck plan's phase 6): a crew of three acts in one world. A research
//! station has stopped transmitting; the crew must find out why and restore contact before its
//! life support runs out, without hurting anyone. Deterministic from its seed, as every
//! scenario (`super::scenario`): the state is the seed's station with the accepted actions
//! replayed, and only these rules say how it ended.
//!
//! The hidden cause is always a burnt-out comms relay, with an anomaly the seed picks: an ion
//! storm on the power junction (routing power through it before it is understood hurts the
//! engineer) or a magnetic field (harmless, but it scatters a call). Seen from outside, the
//! comms array looks intact: misleading, not arbitrary. Two ways succeed: repair the power path
//! and call from the station's own array, or understand the anomaly and place a probe as a
//! relay. The ship's energy pays for one of them in full, not both.

use super::games::Rng;

/// The station's kind, as a scenario names it.
pub const SILENT_STATION: &str = "silent_station";

/// The station's rules, by version: a trial compares runs of one version alone.
pub const VERSION: u32 = 1;

/// The ship's energy at the start.
pub const ENERGY: u32 = 100;

/// The station's life support at the start, in ticks.
pub const LIFE_SUPPORT: u32 = 10;

/// What routing power restores to the station's life support, in ticks.
pub const POWER_RESTORES: u32 = 6;

/// The anomaly the seed hides.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Anomaly {
    /// An ion storm on the power junction: routing power before analysing it arcs.
    IonStorm,
    /// A magnetic field around the station: harmless, but it scatters a call.
    MagneticField,
}

impl Anomaly {
    /// Its name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::IonStorm => "ion storm",
            Self::MagneticField => "magnetic field",
        }
    }
}

/// A mission role, which a member wears for the mission alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// Decides priorities; sends the probe and makes the call.
    Captain,
    /// Scans the station and analyses the anomaly.
    Science,
    /// Inspects the comms and routes power.
    Engineer,
}

impl Role {
    /// Its name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Captain => "captain",
            Self::Science => "science",
            Self::Engineer => "engineer",
        }
    }

    /// From its name.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "captain" => Some(Self::Captain),
            "science" => Some(Self::Science),
            "engineer" => Some(Self::Engineer),
            _ => None,
        }
    }

    /// The actions this role takes.
    #[must_use]
    pub const fn actions(self) -> [Action; 2] {
        match self {
            Self::Captain => [Action::SendProbe, Action::AttemptContact],
            Self::Science => [Action::ScanStation, Action::AnalyzeAnomaly],
            Self::Engineer => [Action::InspectComms, Action::RoutePower],
        }
    }
}

/// What the crew does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Science: life signs, power and hazard, from the ship. 1 tick, 10 energy.
    ScanStation,
    /// Engineer: open the comms junction. 2 ticks, 10 energy; after a scan.
    InspectComms,
    /// Science: what the anomaly is. 2 ticks, 15 energy; after a scan.
    AnalyzeAnomaly,
    /// Engineer: bypass the dead relay. 2 ticks, 25 energy; once the relay is found.
    RoutePower,
    /// Captain: a probe beside the array, as a relay. 1 tick, 35 energy; after a scan.
    SendProbe,
    /// Captain: call the station. 1 tick, 10 energy; after a scan.
    AttemptContact,
}

/// Every action, in the order the tool names them.
pub const ACTIONS: [Action; 6] = [
    Action::ScanStation,
    Action::InspectComms,
    Action::AnalyzeAnomaly,
    Action::RoutePower,
    Action::SendProbe,
    Action::AttemptContact,
];

impl Action {
    /// Its name, as the tool takes it and the log keeps it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::ScanStation => "scan_station",
            Self::InspectComms => "inspect_comms",
            Self::AnalyzeAnomaly => "analyze_anomaly",
            Self::RoutePower => "route_power",
            Self::SendProbe => "send_probe",
            Self::AttemptContact => "attempt_contact",
        }
    }

    /// From its name.
    #[must_use]
    pub fn read(name: &str) -> Option<Self> {
        ACTIONS.into_iter().find(|a| a.name() == name.trim())
    }

    /// What it costs: ticks, then the ship's energy.
    #[must_use]
    pub const fn cost(self) -> (u32, u32) {
        match self {
            Self::ScanStation => (1, 10),
            Self::InspectComms => (2, 10),
            Self::AnalyzeAnomaly => (2, 15),
            Self::RoutePower => (2, 25),
            Self::SendProbe => (1, 35),
            Self::AttemptContact => (1, 10),
        }
    }

    /// The role that takes it.
    #[must_use]
    pub fn role(self) -> Role {
        [Role::Captain, Role::Science, Role::Engineer]
            .into_iter()
            .find(|r| r.actions().contains(&self))
            .unwrap_or(Role::Captain)
    }
}

/// What the crew has found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Clue {
    /// The station is alive, its power low.
    Scanned,
    /// The comms relay in the power junction is burnt out.
    RelayDamaged,
    /// What the anomaly is.
    AnomalyKnown,
}

/// How the mission ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Contact restored, nobody hurt.
    Success,
    /// The cause found, the station not reached in time.
    Partial,
    /// Someone hurt, or time out with the cause unknown.
    Failure,
}

impl Outcome {
    /// Its name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Partial => "partial",
            Self::Failure => "failure",
        }
    }
}

/// The station and the ship, as they stand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Station {
    /// The seed's secret.
    pub anomaly: Anomaly,
    /// The station's power, in percent.
    pub station_power: u32,
    /// Ticks of life support the station has left.
    pub life_support: u32,
    /// The ship's energy.
    pub ship_energy: u32,
    /// Ticks spent.
    pub elapsed: u32,
    /// How dangerous the station is to approach: 0 to 3.
    pub hazard: u32,
    /// Whether nobody has been hurt.
    pub crew_safe: bool,
    /// What has been found, in order.
    pub clues: Vec<Clue>,
    /// Whether power bypasses the dead relay.
    pub power_routed: bool,
    /// Whether a probe holds beside the array.
    pub probe_out: bool,
    /// Whether the station answered.
    pub contact: bool,
}

impl Station {
    /// The station a seed makes.
    #[must_use]
    pub fn new(seed: u64) -> Self {
        let anomaly = if Rng::new(seed).below(2) == 0 {
            Anomaly::IonStorm
        } else {
            Anomaly::MagneticField
        };
        Self {
            anomaly,
            station_power: 20,
            life_support: LIFE_SUPPORT,
            ship_energy: ENERGY,
            elapsed: 0,
            hazard: match anomaly {
                Anomaly::IonStorm => 2,
                Anomaly::MagneticField => 1,
            },
            crew_safe: true,
            clues: Vec::new(),
            power_routed: false,
            probe_out: false,
            contact: false,
        }
    }

    fn knows(&self, clue: Clue) -> bool {
        self.clues.contains(&clue)
    }

    fn learn(&mut self, clue: Clue) {
        if !self.knows(clue) {
            self.clues.push(clue);
        }
    }

    /// How it ended, if it has.
    #[must_use]
    pub fn outcome(&self) -> Option<Outcome> {
        if !self.crew_safe {
            return Some(Outcome::Failure);
        }
        if self.contact {
            return Some(Outcome::Success);
        }
        if self.life_support > 0 {
            return None;
        }
        if self.knows(Clue::RelayDamaged) && self.knows(Clue::AnomalyKnown) {
            return Some(Outcome::Partial);
        }
        Some(Outcome::Failure)
    }

    /// The score the outcome earns: success the sooner the better, the cause alone a little.
    #[must_use]
    pub fn score(&self) -> u32 {
        match self.outcome() {
            Some(Outcome::Success) => 100u32.saturating_sub(5 * self.elapsed).max(50),
            Some(Outcome::Partial) => 30,
            _ => 0,
        }
    }

    /// One action by a member in `role`: the station after it and what the crew sees, or why
    /// it is refused. A refused action changes nothing.
    ///
    /// # Errors
    /// Why the action cannot be done now, or not by this role.
    pub fn step(&self, role: Role, action: Action) -> Result<(Self, String), String> {
        if let Some(outcome) = self.outcome() {
            return Err(format!("the mission is over: {}", outcome.name()));
        }
        if action.role() != role {
            return Err(format!(
                "{} is the {}'s to do, not the {}'s",
                action.name(),
                action.role().name(),
                role.name()
            ));
        }
        if action != Action::ScanStation && !self.knows(Clue::Scanned) {
            return Err("nobody has scanned the station yet: science scans it first".to_owned());
        }
        if action == Action::RoutePower && !self.knows(Clue::RelayDamaged) {
            return Err("nobody knows where the fault is: inspect the comms first".to_owned());
        }
        if action == Action::RoutePower && self.power_routed {
            return Err("power already bypasses the relay".to_owned());
        }
        if action == Action::SendProbe && self.probe_out {
            return Err("a probe already holds beside the array".to_owned());
        }
        let (ticks, energy) = action.cost();
        if self.ship_energy < energy {
            return Err(format!(
                "the ship has {} energy and {} needs {energy}",
                self.ship_energy,
                action.name()
            ));
        }
        let mut next = self.clone();
        next.elapsed += ticks;
        next.ship_energy -= energy;
        next.life_support = next.life_support.saturating_sub(ticks);
        let seen = next.apply(action);
        let said = match next.outcome() {
            Some(outcome) => format!("{seen} The mission is over: {}.", outcome.name()),
            None => format!(
                "{seen} {} ticks of life support left; ship energy {}.",
                next.life_support, next.ship_energy
            ),
        };
        Ok((next, said))
    }

    /// What an accepted action does, and what is seen.
    fn apply(&mut self, action: Action) -> String {
        match action {
            Action::ScanStation => {
                self.learn(Clue::Scanned);
                format!(
                    "Four life signs. Station power {}%. Hazard {} of 3. The comms array looks \
                     intact from outside.",
                    self.station_power, self.hazard
                )
            }
            Action::InspectComms => {
                self.learn(Clue::RelayDamaged);
                "The comms relay in the power junction is burnt out; the array itself is fine."
                    .to_owned()
            }
            Action::AnalyzeAnomaly => {
                self.learn(Clue::AnomalyKnown);
                self.hazard = 1;
                match self.anomaly {
                    Anomaly::IonStorm => "An ion storm sits on the power junction. Shielded now, \
                        the engineer can work there; unshielded, power through it would arc."
                        .to_owned(),
                    Anomaly::MagneticField => "A magnetic field wraps the station: harmless to \
                        the crew, but it scatters a call from outside it."
                        .to_owned(),
                }
            }
            Action::RoutePower => {
                if self.anomaly == Anomaly::IonStorm && !self.knows(Clue::AnomalyKnown) {
                    self.crew_safe = false;
                    return "The junction arcs through the storm: the engineer is hurt.".to_owned();
                }
                self.power_routed = true;
                self.station_power = 60;
                self.life_support += POWER_RESTORES;
                "Power bypasses the dead relay. Station power 60%; life support recovers."
                    .to_owned()
            }
            Action::SendProbe => {
                if !self.knows(Clue::AnomalyKnown) {
                    return "The probe drifts into the anomaly and goes quiet: lost.".to_owned();
                }
                self.probe_out = true;
                "The probe holds beside the array, clear of the anomaly: a relay for a call."
                    .to_owned()
            }
            Action::AttemptContact => {
                let reaches =
                    self.probe_out || (self.power_routed && self.anomaly == Anomaly::IonStorm);
                if reaches {
                    self.contact = true;
                    return "The station answers: they are cold and frightened, and alive."
                        .to_owned();
                }
                if self.power_routed {
                    return "A scattered reply, nothing clear: something bends the signal."
                        .to_owned();
                }
                "Static.".to_owned()
            }
        }
    }

    /// The station with `actions`, each by its role, applied in order: what a log replays to.
    ///
    /// # Errors
    /// The first action that is refused, which a kept log never holds.
    pub fn replay(seed: u64, actions: &[Action]) -> Result<Self, String> {
        actions.iter().try_fold(Self::new(seed), |at, action| {
            Ok(at.step(action.role(), *action)?.0)
        })
    }

    /// Where things stand, said: never the anomaly until it is analysed.
    #[must_use]
    pub fn look(&self) -> String {
        let found: Vec<&str> = self
            .clues
            .iter()
            .map(|c| match c {
                Clue::Scanned => "the station is alive, its power low",
                Clue::RelayDamaged => "the comms relay is burnt out",
                Clue::AnomalyKnown => match self.anomaly {
                    Anomaly::IonStorm => "an ion storm on the power junction",
                    Anomaly::MagneticField => "a magnetic field that scatters calls",
                },
            })
            .collect();
        let state = match self.outcome() {
            Some(outcome) => format!("The mission is over: {}.", outcome.name()),
            None => "The station is silent.".to_owned(),
        };
        format!(
            "{state} Life support {} ticks; ship energy {}; station power {}%; hazard {} of 3; \
             {} ticks spent. Found: {}. Power routed: {}. Probe out: {}.",
            self.life_support,
            self.ship_energy,
            self.station_power,
            self.hazard,
            self.elapsed,
            if found.is_empty() {
                "nothing yet".to_owned()
            } else {
                found.join("; ")
            },
            if self.power_routed { "yes" } else { "no" },
            if self.probe_out { "yes" } else { "no" },
        )
    }
}

/// What the crew is told before it acts: the mission's description, with the member's role.
#[must_use]
pub fn brief(role: Role) -> String {
    let yours = role
        .actions()
        .iter()
        .map(|a| a.name())
        .collect::<Vec<_>>()
        .join(" and ");
    format!(
        "Silent Station. A research station has stopped transmitting. Your ship has {ENERGY} \
         energy; the station has {LIFE_SUPPORT} ticks of life support. Find out why it is \
         silent and restore contact without hurting anyone. You are the {role} for this \
         mission: your actions are {yours}, with the scenario tool; look (free) shows what the \
         crew has found. The captain sends the probe and makes the call, science scans and \
         analyses, the engineer inspects the comms and routes power. Everything that happens \
         is the world's word, and the world closes this mission.",
        role = role.name()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed_for(anomaly: Anomaly) -> u64 {
        (0..).find(|s| Station::new(*s).anomaly == anomaly).unwrap()
    }

    use Action::{
        AnalyzeAnomaly, AttemptContact, InspectComms, RoutePower, ScanStation, SendProbe,
    };

    #[test]
    fn the_seed_picks_the_anomaly_and_hides_it_until_analysed() {
        let seeds: Vec<Anomaly> = (0..20).map(|s| Station::new(s).anomaly).collect();
        assert!(seeds.contains(&Anomaly::IonStorm) && seeds.contains(&Anomaly::MagneticField));
        assert_eq!(Station::new(9), Station::new(9));
        let storm = Station::new(seed_for(Anomaly::IonStorm));
        assert!(!storm.look().contains("storm"));
        let (scanned, said) = storm.step(Role::Science, ScanStation).unwrap();
        assert!(
            said.contains("looks intact"),
            "misleading, not arbitrary: {said}"
        );
        let (known, _) = scanned.step(Role::Science, AnalyzeAnomaly).unwrap();
        assert!(known.look().contains("ion storm"));
    }

    #[test]
    fn two_ways_succeed_and_energy_pays_for_one() {
        // The probe as a relay, for either anomaly.
        for anomaly in [Anomaly::IonStorm, Anomaly::MagneticField] {
            let done = Station::replay(
                seed_for(anomaly),
                &[ScanStation, AnalyzeAnomaly, SendProbe, AttemptContact],
            )
            .unwrap();
            assert_eq!(done.outcome(), Some(Outcome::Success), "{anomaly:?}");
            assert_eq!((done.elapsed, done.score()), (5, 75));
        }
        // The repair, safely, through the storm.
        let repaired = Station::replay(
            seed_for(Anomaly::IonStorm),
            &[
                ScanStation,
                InspectComms,
                AnalyzeAnomaly,
                RoutePower,
                AttemptContact,
            ],
        )
        .unwrap();
        assert_eq!(repaired.outcome(), Some(Outcome::Success));
        assert_eq!(repaired.ship_energy, 30);
        let routed = Station::replay(
            seed_for(Anomaly::IonStorm),
            &[ScanStation, InspectComms, AnalyzeAnomaly, RoutePower],
        )
        .unwrap();
        assert_eq!(routed.ship_energy, 40);
        let (probed, _) = routed.step(Role::Captain, SendProbe).unwrap();
        assert_eq!(
            probed.step(Role::Captain, AttemptContact).unwrap_err(),
            "the ship has 5 energy and attempt_contact needs 10",
            "both ways leave nothing for the call"
        );
    }

    #[test]
    fn rushing_the_storm_hurts_the_engineer_and_a_field_scatters_the_call() {
        let hurt = Station::replay(
            seed_for(Anomaly::IonStorm),
            &[ScanStation, InspectComms, RoutePower],
        )
        .unwrap();
        assert_eq!(hurt.outcome(), Some(Outcome::Failure));
        assert!(!hurt.crew_safe);
        let field = Station::replay(
            seed_for(Anomaly::MagneticField),
            &[ScanStation, InspectComms, RoutePower],
        )
        .unwrap();
        let (scattered, said) = field.step(Role::Captain, AttemptContact).unwrap();
        assert!(
            said.contains("scattered") && scattered.outcome().is_none(),
            "{said}"
        );
    }

    #[test]
    fn a_refused_action_changes_nothing_and_each_role_acts_alone() {
        let start = Station::new(seed_for(Anomaly::MagneticField));
        assert!(
            start
                .step(Role::Captain, AttemptContact)
                .unwrap_err()
                .contains("scan")
        );
        assert_eq!(
            start.step(Role::Captain, ScanStation).unwrap_err(),
            "scan_station is the science's to do, not the captain's"
        );
        let (scanned, _) = start.step(Role::Science, ScanStation).unwrap();
        assert!(
            scanned
                .step(Role::Engineer, RoutePower)
                .unwrap_err()
                .contains("inspect the comms")
        );
        assert_eq!(scanned.elapsed, 1, "refusals cost nothing");
    }

    #[test]
    fn time_out_is_partial_with_the_cause_found_and_a_failure_without() {
        let mut at = Station::replay(
            seed_for(Anomaly::MagneticField),
            &[ScanStation, InspectComms, AnalyzeAnomaly],
        )
        .unwrap();
        while at.outcome().is_none() {
            at = at.step(Role::Captain, AttemptContact).unwrap().0;
        }
        assert_eq!((at.outcome(), at.score()), (Some(Outcome::Partial), 30));
        let mut blind = Station::new(seed_for(Anomaly::MagneticField));
        blind = blind.step(Role::Science, ScanStation).unwrap().0;
        while blind.outcome().is_none() {
            blind = blind.step(Role::Captain, AttemptContact).unwrap().0;
        }
        assert_eq!(
            (blind.outcome(), blind.score()),
            (Some(Outcome::Failure), 0)
        );
    }

    #[test]
    fn every_action_reads_back_and_belongs_to_one_role() {
        for action in ACTIONS {
            assert_eq!(Action::read(action.name()), Some(action));
            assert!(action.role().actions().contains(&action));
        }
        assert_eq!(Action::read("fire_phasers"), None);
        assert!(brief(Role::Engineer).contains("inspect_comms and route_power"));
    }
}
