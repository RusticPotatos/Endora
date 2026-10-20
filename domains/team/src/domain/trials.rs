//! Trials (the Holodeck plan's phase 7): the same scenario, on the same seeds, played under two
//! or more conditions (who plays it, on which brains), and compared. One thing changes between
//! conditions; the world, its version and its seeds do not. A trial's summary is arithmetic
//! over the record (the scenario's log and the turns that played it), never a model's account.

/// The most runs one trial starts: enough to compare, few enough to read.
pub const MOST_RUNS: usize = 24;

/// One run of a trial, as the record shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    /// The condition it ran under.
    pub condition: String,
    /// The scenario's task.
    pub task: i64,
    /// Its seed.
    pub seed: u64,
    /// How it ended (`success`, `partial`, `failure`, `solved`, `failed`), if it has.
    pub end: Option<String>,
    /// Whether that end is the work done.
    pub done: bool,
    /// What it scored: a lighthouse 100 solved and 0 failed; a mission its own score.
    pub score: u32,
    /// Time spent in the world, in its own unit.
    pub elapsed: u32,
    /// Actions the world accepted.
    pub accepted: u32,
    /// Actions it refused.
    pub refused: u32,
    /// Whether anyone was hurt.
    pub unsafe_act: bool,
    /// Turns taken on it.
    pub turns: u32,
    /// Of those, turns that changed nothing.
    pub idle_turns: u32,
    /// What its turns cost, in micro-dollars.
    pub spent: i64,
    /// The brains its turns ran on, each once.
    pub brains: Vec<String>,
}

/// One condition of a trial, summed over its runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Condition {
    /// Its label.
    pub label: String,
    /// Its runs.
    pub runs: u32,
    /// Runs that ended with the work done.
    pub done: u32,
    /// Runs that ended otherwise.
    pub not_done: u32,
    /// Runs not over yet.
    pub unfinished: u32,
    /// The mean score of the runs that ended, rounded down.
    pub mean_score: u32,
    /// The mean time spent by the runs that ended, rounded down.
    pub mean_elapsed: u32,
    /// Actions refused, over all its runs.
    pub refused: u32,
    /// Runs in which someone was hurt.
    pub unsafe_runs: u32,
    /// Turns taken, over all its runs.
    pub turns: u32,
    /// Turns that changed nothing.
    pub idle_turns: u32,
    /// What its turns cost, in micro-dollars.
    pub spent: i64,
    /// The brains its turns ran on, each once.
    pub brains: Vec<String>,
}

/// The conditions of a trial, in the order their first run appears.
#[must_use]
pub fn summarize(runs: &[Run]) -> Vec<Condition> {
    let mut labels: Vec<&str> = Vec::new();
    for run in runs {
        if !labels.contains(&run.condition.as_str()) {
            labels.push(&run.condition);
        }
    }
    labels
        .into_iter()
        .map(|label| {
            let mine: Vec<&Run> = runs.iter().filter(|r| r.condition == label).collect();
            let ended: Vec<&&Run> = mine.iter().filter(|r| r.end.is_some()).collect();
            let mean = |of: fn(&Run) -> u32| {
                if ended.is_empty() {
                    return 0;
                }
                let total: u64 = ended.iter().map(|r| u64::from(of(r))).sum();
                u32::try_from(total / ended.len() as u64).unwrap_or(u32::MAX)
            };
            let count = |keep: fn(&Run) -> bool| {
                u32::try_from(mine.iter().filter(|r| keep(r)).count()).unwrap_or(u32::MAX)
            };
            let sum = |of: fn(&Run) -> u32| mine.iter().map(|r| of(r)).sum::<u32>();
            let mut brains: Vec<String> = Vec::new();
            for brain in mine.iter().flat_map(|r| r.brains.iter()) {
                if !brains.contains(brain) {
                    brains.push(brain.clone());
                }
            }
            Condition {
                label: label.to_owned(),
                runs: count(|_| true),
                done: count(|r| r.end.is_some() && r.done),
                not_done: count(|r| r.end.is_some() && !r.done),
                unfinished: count(|r| r.end.is_none()),
                mean_score: mean(|r| r.score),
                mean_elapsed: mean(|r| r.elapsed),
                refused: sum(|r| r.refused),
                unsafe_runs: count(|r| r.unsafe_act),
                turns: sum(|r| r.turns),
                idle_turns: sum(|r| r.idle_turns),
                spent: mine.iter().map(|r| r.spent).sum(),
                brains,
            }
        })
        .collect()
}

/// Dollars from micro-dollars, to the cent.
fn dollars(micros: i64) -> String {
    format!(
        "${}.{:02}",
        micros / 1_000_000,
        (micros % 1_000_000) / 10_000
    )
}

/// A trial as a short report to keep or share: what was compared, the conditions side by
/// side, every run, and what the numbers cannot say.
#[must_use]
pub fn report(name: &str, kind: &str, version: u32, runs: &[Run]) -> String {
    let mut out = format!(
        "# Trial {name}\n\n{kind}, version {version}; {} runs. Each condition played the same \
         seeds; only who played changed.\n\n| Condition | Runs | Done | Not done | Unfinished \
         | Mean score | Mean time | Refused | Unsafe | Turns | Idle turns | Cost | Brains |\n\
         |---|---|---|---|---|---|---|---|---|---|---|---|---|\n",
        runs.len()
    );
    for c in summarize(runs) {
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |\n",
            c.label,
            c.runs,
            c.done,
            c.not_done,
            c.unfinished,
            c.mean_score,
            c.mean_elapsed,
            c.refused,
            c.unsafe_runs,
            c.turns,
            c.idle_turns,
            dollars(c.spent),
            c.brains.join(", ")
        ));
    }
    out.push_str("\n## Runs\n\n");
    for r in runs {
        out.push_str(&format!(
            "- {} on seed {}: task {}, {}, score {}, time {}, {} accepted and {} refused, {} \
             turns.\n",
            r.condition,
            r.seed,
            r.task,
            r.end.as_deref().unwrap_or("not over"),
            r.score,
            r.elapsed,
            r.accepted,
            r.refused,
            r.turns
        ));
    }
    out.push_str(
        "\nA few runs a condition show a difference, not a reason: the same model may choose \
         otherwise on the same seed. Read each run's log before believing an explanation.\n",
    );
    out
}

/// A trial's name, if it is one: lowercase letters, digits and dashes, 1 to 40 of them.
#[must_use]
pub fn name_ok(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 40
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// One condition as the owner sets it: its label, and who plays (one member for a
/// lighthouse; the captain, science and the engineer for Silent Station).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Arm {
    /// Its label.
    pub label: String,
    /// Who plays it.
    pub members: Vec<super::work::MemberId>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(condition: &str, end: Option<&str>, score: u32, turns: u32) -> Run {
        Run {
            condition: condition.to_owned(),
            task: 1,
            seed: 7,
            end: end.map(str::to_owned),
            done: end == Some("success"),
            score,
            elapsed: 6,
            accepted: 4,
            refused: 2,
            unsafe_act: end == Some("failure"),
            turns,
            idle_turns: 1,
            spent: 25_000,
            brains: vec!["local".to_owned()],
        }
    }

    #[test]
    fn conditions_are_summed_from_their_runs_and_only_ended_runs_are_averaged() {
        let runs = [
            run("flat", Some("success"), 75, 4),
            run("captain-led", Some("failure"), 0, 6),
            run("flat", Some("partial"), 30, 5),
            run("flat", None, 0, 2),
        ];
        let summed = summarize(&runs);
        assert_eq!(
            summed.iter().map(|c| c.label.as_str()).collect::<Vec<_>>(),
            ["flat", "captain-led"]
        );
        let flat = &summed[0];
        assert_eq!(
            (flat.runs, flat.done, flat.not_done, flat.unfinished),
            (3, 1, 1, 1)
        );
        assert_eq!((flat.mean_score, flat.mean_elapsed), (52, 6));
        assert_eq!(
            (flat.refused, flat.turns, flat.idle_turns, flat.spent),
            (6, 11, 3, 75_000)
        );
        assert_eq!(flat.brains, ["local"]);
        assert_eq!(summed[1].unsafe_runs, 1);
    }

    #[test]
    fn the_report_shows_the_conditions_side_by_side_and_says_what_it_cannot() {
        let said = report(
            "crew-size",
            "silent_station",
            1,
            &[
                run("flat", Some("success"), 75, 4),
                run("captain-led", None, 0, 1),
            ],
        );
        assert!(said.starts_with("# Trial crew-size\n"), "{said}");
        assert!(said.contains("| flat | 1 | 1 | 0 | 0 | 75 | 6 | 2 | 0 | 4 | 1 | $0.02 | local |"));
        assert!(said.contains("- captain-led on seed 7: task 1, not over"));
        assert!(said.contains("not a reason"));
    }

    #[test]
    fn a_trials_name_is_short_and_plain() {
        for good in ["crew-size", "a", "local-vs-haiku-2"] {
            assert!(name_ok(good), "{good}");
        }
        for bad in ["", "Crew", "two words", "x/y", &"a".repeat(41)] {
            assert!(!name_ok(bad), "{bad}");
        }
    }
}
