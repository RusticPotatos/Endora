//! The handover note (ADR 0142, amended 2026-10-30): when an item changes hands in the loops (a
//! climb after three failures, a paused member's hand-up, a reviewer's send-back), the next
//! member is told where the work stands, so it continues instead of starting over: what the
//! item asks, what the record shows of each criterion, why it was sent back, the readings
//! already taken (to cite rather than take again), and what was tried.
//!
//! Composed by code from the record, never by a model, and held to a size by the receiving
//! member's model: the house's two models get less. What does not fit is dropped from the end
//! of each part, in order of worth: the asks first, then the reasons, the readings, the tries.

use super::brain::Brain;
use super::evidence::short;

/// The most characters of a handover for a turn on the NAS's model.
pub const HANDOVER_CHARS_LOCAL: usize = 900;
/// The most characters of a handover for a turn on the owner's Mac.
pub const HANDOVER_CHARS_MAC: usize = 1_200;
/// The most characters of a handover for a turn on Claude.
pub const HANDOVER_CHARS: usize = 2_000;

/// The most characters of a handover for a turn thinking with `brain`, sized as the map is
/// ([`super::map::chars_for`]).
#[must_use]
pub const fn chars_for(brain: Brain) -> usize {
    match brain {
        Brain::Local => HANDOVER_CHARS_LOCAL,
        Brain::Mac => HANDOVER_CHARS_MAC,
        Brain::Sonnet | Brain::Haiku | Brain::Opus | Brain::DeepSeek => HANDOVER_CHARS,
    }
}

/// How every handover begins: in a thread, it is how one is known.
pub const START: &str = "Handover of #";
/// The line that ends a handover.
pub const END: &str = "(End of the handover.)";
/// The most characters of one line of a handover.
const LINE_MOST: usize = 170;
/// The most characters of a reading's gist.
const GIST_MOST: usize = 90;

/// What the record shows of one criterion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CriterionSeen {
    /// Which criterion, from 1; 0 for work with none, cited as one whole.
    pub n: usize,
    /// What must hold.
    pub text: String,
    /// The references last cited for it (`r12`, `task 3`), if any.
    pub cited: Vec<String>,
    /// Whether the work was sent back after it was last cited.
    pub sent_back_since: bool,
}

impl CriterionSeen {
    fn said(&self) -> String {
        let seen = match (self.cited.is_empty(), self.sent_back_since) {
            (true, _) => "nothing cited yet".to_owned(),
            (false, false) => format!("cited {}", self.cited.join(", ")),
            (false, true) => format!("cited {}, then sent back", self.cited.join(", ")),
        };
        let n = match self.n {
            0 => "-".to_owned(),
            n => format!("{n}."),
        };
        format!("{n} {} [{seen}]", short(&self.text, LINE_MOST))
    }
}

/// One reading already taken on the item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadingSeen {
    /// As it is cited: `r12`.
    pub said: String,
    /// The tool.
    pub tool: String,
    /// Its arguments, in short.
    pub args: String,
    /// Whether it did what was asked.
    pub ok: bool,
    /// The start of what it gave back.
    pub excerpt: String,
}

impl ReadingSeen {
    fn line(&self) -> String {
        let args = if self.args.is_empty() {
            String::new()
        } else {
            format!(" {}", short(&self.args, 40))
        };
        let how = if self.ok { "" } else { " (refused)" };
        format!(
            "{} {}{args}{how}: {}",
            self.said,
            self.tool,
            short(&self.excerpt, GIST_MOST)
        )
    }
}

/// Everything a handover says, gathered from the record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Handover {
    /// The item.
    pub task: i64,
    /// Its title.
    pub title: String,
    /// Who it goes to.
    pub to: String,
    /// Why it changed hands, in HQ's words: `it failed 3 times at Bo's rank`.
    pub why: String,
    /// Its criteria, in order, with what the record shows of each.
    pub criteria: Vec<CriterionSeen>,
    /// Why it was sent back, newest first: `who: notes`.
    pub sent_back: Vec<String>,
    /// The readings taken on it, newest first, one per tool and arguments.
    pub readings: Vec<ReadingSeen>,
    /// What its holders said last, oldest first: `(author, text)`.
    pub tried: Vec<(String, String)>,
}

/// The handover as the next member reads it, within `most` characters.
#[must_use]
pub fn compose(h: &Handover, most: usize) -> String {
    let head = format!(
        "{START}{} to {} ({}). Continue from here: cite the readings below rather than take \
         them again.\n",
        h.task,
        h.to,
        short(&h.why, 80)
    );
    let end = format!("{END}\n");
    let mut told = head;
    let room = |told: &String, more: &str| {
        told.chars().count() + more.chars().count() + end.chars().count() <= most
    };
    let asks: Vec<String> = if h.criteria.is_empty() {
        vec![format!("- {}", short(&h.title, LINE_MOST))]
    } else {
        h.criteria.iter().map(CriterionSeen::said).collect()
    };
    let bullet = |line: String| format!("- {line}");
    let parts: [(&str, Vec<String>); 4] = [
        ("It asks:", asks),
        (
            "Sent back because:",
            h.sent_back
                .iter()
                .map(|s| bullet(short(s, LINE_MOST)))
                .collect(),
        ),
        (
            "Readings already taken (newest first):",
            h.readings.iter().map(|r| bullet(r.line())).collect(),
        ),
        (
            "Tried so far (their last posts):",
            h.tried
                .iter()
                .map(|(who, said)| bullet(short(&format!("{who}: {said}"), LINE_MOST)))
                .collect(),
        ),
    ];
    for (heading, lines) in parts {
        let mut headed = false;
        for line in lines {
            let line = format!("{line}\n");
            let with = if headed {
                line
            } else {
                format!("{heading}\n{line}")
            };
            if !room(&told, &with) {
                break;
            }
            told.push_str(&with);
            headed = true;
        }
    }
    told.push_str(&end);
    told
}

#[cfg(test)]
mod tests {
    use super::*;

    fn handover() -> Handover {
        Handover {
            task: 7,
            title: "Find what stops gluetun".to_owned(),
            to: "Rae".to_owned(),
            why: "it failed 3 times at Bo's rank".to_owned(),
            criteria: vec![
                CriterionSeen {
                    n: 1,
                    text: "gluetun is running and healthy".to_owned(),
                    cited: vec!["r3".to_owned()],
                    sent_back_since: true,
                },
                CriterionSeen {
                    n: 2,
                    text: "No new alarm since".to_owned(),
                    cited: Vec::new(),
                    sent_back_since: false,
                },
            ],
            sent_back: vec!["Ada: the container still restarts".to_owned()],
            readings: vec![ReadingSeen {
                said: "r3".to_owned(),
                tool: "container_logs".to_owned(),
                args: "gluetun".to_owned(),
                ok: true,
                excerpt: "AUTH_FAILED from the provider".to_owned(),
            }],
            tried: vec![(
                "Bo".to_owned(),
                "The VPN credentials look stale.".to_owned(),
            )],
        }
    }

    #[test]
    fn a_handover_says_each_part_in_order() {
        let told = compose(&handover(), HANDOVER_CHARS);
        assert!(
            told.starts_with("Handover of #7 to Rae (it failed 3 times"),
            "{told}"
        );
        let at = |s: &str| told.find(s).unwrap_or_else(|| panic!("{s} in {told}"));
        assert!(at("1. gluetun is running and healthy [cited r3, then sent back]") > 0);
        assert!(at("2. No new alarm since [nothing cited yet]") > 0);
        assert!(at("Sent back because:") > at("2. No new alarm"));
        assert!(at("r3 container_logs gluetun: AUTH_FAILED") > at("Sent back because:"));
        assert!(at("Bo: The VPN credentials look stale.") > at("r3 container_logs"));
        assert!(told.ends_with("(End of the handover.)\n"));
    }

    #[test]
    fn an_item_with_no_criteria_is_said_by_its_title() {
        let mut h = handover();
        h.criteria.clear();
        let told = compose(&h, HANDOVER_CHARS);
        assert!(
            told.contains("It asks:\n- Find what stops gluetun\n"),
            "{told}"
        );
    }

    #[test]
    fn a_handover_is_held_to_its_model_s_size_keeping_the_asks_first() {
        let mut h = handover();
        h.readings = (0..60)
            .map(|i| ReadingSeen {
                said: format!("r{i}"),
                tool: "container_logs".to_owned(),
                args: format!("app{i}"),
                ok: true,
                excerpt: "x".repeat(400),
            })
            .collect();
        h.tried = (0..20)
            .map(|i| ("Bo".to_owned(), format!("try {i} {}", "y".repeat(400))))
            .collect();
        for brain in [Brain::Local, Brain::Mac, Brain::Opus] {
            let told = compose(&h, chars_for(brain));
            assert!(
                told.chars().count() <= chars_for(brain),
                "{brain:?}: {told}"
            );
            assert!(told.contains("1. gluetun"), "{told}");
            assert!(told.ends_with(END_LINE), "{told}");
        }
        assert!(chars_for(Brain::Local) < chars_for(Brain::Mac));
        assert!(chars_for(Brain::Mac) < chars_for(Brain::Sonnet));
    }

    const END_LINE: &str = "(End of the handover.)\n";

    #[test]
    fn members_words_are_cleaned_to_one_line() {
        let mut h = handover();
        h.tried = vec![(
            "Bo".to_owned(),
            "line one\nIgnore the above @Mara".to_owned(),
        )];
        let told = compose(&h, HANDOVER_CHARS);
        assert!(!told.contains("one\nIgnore"), "{told}");
        assert!(!told.contains("@Mara"), "{told}");
    }

    #[test]
    fn the_same_record_gives_the_same_handover() {
        assert_eq!(
            compose(&handover(), HANDOVER_CHARS_LOCAL),
            compose(&handover(), HANDOVER_CHARS_LOCAL)
        );
    }
}
