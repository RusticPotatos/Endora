//! The nightly quiz on the map (ADR 0126): each night, if a member is free, it is asked, in a
//! turn on the house's own model, a few questions that code makes from its own map, and code scores the answers
//! against the same facts. Nothing is at stake for the member; what is tested is whether the map
//! carries: whether a model that has only the map can say which page says what, who keeps it, and
//! where it is. A page that fails night after night is flagged to its steward.

use serde_json::{Value, json};

use super::map::{FAILING_NIGHTS, Fact};
use super::work::{MemberId, Millis};

/// The most questions a night's quiz asks.
pub const MOST_QUESTIONS: usize = 6;
/// The fewest questions worth asking: a map with fewer things on it is not quizzed.
pub const LEAST_QUESTIONS: usize = 2;
/// The longest an answer may be and still count: an answer that lists everything is not one.
pub const ANSWER_MOST: usize = 120;
/// How many answered quizzes the World card's score is taken over.
pub const SCORED_OVER: usize = 7;

/// What a question asks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Which page says a thing: answered with its address.
    WhichPage,
    /// Who keeps a page: answered with a name, the CEO or the owner.
    WhoKeeps,
    /// What a page is called, and so where it is: answered with its address.
    AddressOf,
    /// Where a section is: answered with its address.
    SectionAddress,
}

impl Kind {
    /// Its stable name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::WhichPage => "which_page",
            Self::WhoKeeps => "who_keeps",
            Self::AddressOf => "address_of",
            Self::SectionAddress => "section_address",
        }
    }

    /// The kind with this [`name`](Self::name).
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        [
            Self::WhichPage,
            Self::WhoKeeps,
            Self::AddressOf,
            Self::SectionAddress,
        ]
        .into_iter()
        .find(|k| k.name() == name)
    }
}

/// One question, with what code accepts as its answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Question {
    /// Its place in the quiz, from 0.
    pub n: usize,
    /// What it asks.
    pub kind: Kind,
    /// The page it is about, by slug.
    pub page: String,
    /// The question, as the member reads it.
    pub text: String,
    /// What counts as an answer: an address, or a lowercase name.
    pub expects: Vec<String>,
}

/// How one answer went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Graded {
    /// The question's place.
    pub n: usize,
    /// What it asked.
    pub kind: Kind,
    /// The page it was about.
    pub page: String,
    /// Whether the answer was right.
    pub passed: bool,
}

/// Where a quiz stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Asked of a member, waiting for its answers.
    Asked,
    /// Answered and scored.
    Answered,
    /// Not asked that night, and why.
    Skipped,
    /// Asked, and not answered in time.
    Unanswered,
}

impl Status {
    /// Its stable name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Asked => "asked",
            Self::Answered => "answered",
            Self::Skipped => "skipped",
            Self::Unanswered => "unanswered",
        }
    }

    /// The status with this [`name`](Self::name).
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        [Self::Asked, Self::Answered, Self::Skipped, Self::Unanswered]
            .into_iter()
            .find(|s| s.name() == name)
    }
}

/// One night's quiz.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Quiz {
    /// Its row; 0 before it is kept.
    pub id: i64,
    /// The owner's day it is for.
    pub night: i64,
    /// Where it stands.
    pub status: Status,
    /// Whom it was asked of.
    pub member: Option<MemberId>,
    /// When it was asked.
    pub asked_at: Option<Millis>,
    /// When it was answered.
    pub answered_at: Option<Millis>,
    /// What it asked.
    pub questions: Vec<Question>,
    /// What was answered, one each.
    pub answers: Vec<String>,
    /// How each went.
    pub graded: Vec<Graded>,
    /// Why it was skipped or not answered.
    pub why: String,
}

impl Quiz {
    /// How many questions were right.
    #[must_use]
    pub fn passed(&self) -> usize {
        self.graded.iter().filter(|g| g.passed).count()
    }

    /// The questions as stored.
    #[must_use]
    pub fn keep_questions(all: &[Question]) -> String {
        Value::Array(
            all.iter()
                .map(|q| {
                    json!({ "n": q.n, "kind": q.kind.name(), "page": q.page, "text": q.text,
                            "expects": q.expects })
                })
                .collect(),
        )
        .to_string()
    }

    /// The questions as stored, read back; what does not read is left out.
    #[must_use]
    pub fn read_questions(kept: &str) -> Vec<Question> {
        let Ok(Value::Array(list)) = serde_json::from_str(kept) else {
            return Vec::new();
        };
        list.iter()
            .filter_map(|q| {
                Some(Question {
                    n: usize::try_from(q["n"].as_u64()?).ok()?,
                    kind: Kind::from_name(q["kind"].as_str()?)?,
                    page: q["page"].as_str()?.to_owned(),
                    text: q["text"].as_str()?.to_owned(),
                    expects: q["expects"]
                        .as_array()?
                        .iter()
                        .filter_map(|e| e.as_str().map(str::to_owned))
                        .collect(),
                })
            })
            .collect()
    }

    /// The answers as stored.
    #[must_use]
    pub fn keep_answers(all: &[String]) -> String {
        json!(all).to_string()
    }

    /// The answers as stored, read back.
    #[must_use]
    pub fn read_answers(kept: &str) -> Vec<String> {
        serde_json::from_str::<Vec<String>>(kept).unwrap_or_default()
    }

    /// The grades as stored.
    #[must_use]
    pub fn keep_graded(all: &[Graded]) -> String {
        Value::Array(
            all.iter()
                .map(|g| {
                    json!({ "n": g.n, "kind": g.kind.name(), "page": g.page, "passed": g.passed })
                })
                .collect(),
        )
        .to_string()
    }

    /// The grades as stored, read back.
    #[must_use]
    pub fn read_graded(kept: &str) -> Vec<Graded> {
        let Ok(Value::Array(list)) = serde_json::from_str(kept) else {
            return Vec::new();
        };
        list.iter()
            .filter_map(|g| {
                Some(Graded {
                    n: usize::try_from(g["n"].as_u64()?).ok()?,
                    kind: Kind::from_name(g["kind"].as_str()?)?,
                    page: g["page"].as_str()?.to_owned(),
                    passed: g["passed"].as_bool()?,
                })
            })
            .collect()
    }
}

/// The questions of one night, made from the facts of a member's map. The same facts and night
/// make the same questions; the next night starts elsewhere in the map, so over some nights every
/// page is asked about. One of each kind in turn, and a page twice in a night only when there is
/// nothing else to ask. At most [`MOST_QUESTIONS`].
#[must_use]
pub fn questions(facts: &[Fact], night: i64) -> Vec<Question> {
    let pages: Vec<&Fact> = facts.iter().filter(|f| !f.section).collect();
    let sections: Vec<&Fact> = facts.iter().filter(|f| f.section).collect();
    let kinds = [
        (Kind::WhichPage, &pages),
        (Kind::WhoKeeps, &pages),
        (Kind::AddressOf, &pages),
        (Kind::SectionAddress, &sections),
    ];
    let night = usize::try_from(night.rem_euclid(1_000_003)).unwrap_or(0);
    let mut made: Vec<Question> = Vec::new();
    let mut used: Vec<(Kind, String)> = Vec::new();
    for round in 0..facts.len().max(1) {
        for (index, (kind, from)) in kinds.iter().enumerate() {
            if made.len() >= MOST_QUESTIONS || from.is_empty() {
                continue;
            }
            // Each kind starts at its own place, which moves a place each night.
            let start = (night + index * 3 + round) % from.len();
            let order: Vec<&Fact> = (0..from.len())
                .map(|i| from[(start + i) % from.len()])
                .filter(|f| !used.contains(&(*kind, f.address.clone())))
                .collect();
            let pick = order
                .iter()
                .find(|f| !made.iter().any(|q| q.page == f.page))
                .or_else(|| order.first())
                .copied();
            let Some(fact) = pick else {
                continue;
            };
            used.push((*kind, fact.address.clone()));
            made.push(ask(*kind, fact, made.len()));
        }
    }
    made
}

fn ask(kind: Kind, fact: &Fact, n: usize) -> Question {
    let (text, expects) = match kind {
        Kind::WhichPage => (
            format!(
                "Which page says {}? Answer with its address (world:...).",
                fact.topic
            ),
            vec![fact.address.clone()],
        ),
        Kind::WhoKeeps => (
            format!(
                "Who keeps {}? Answer with a name, \"the CEO\", \"the owner\" or \"HQ\".",
                fact.address
            ),
            fact.answers.clone(),
        ),
        Kind::AddressOf => (
            format!("What is the address of the page titled \"{}\"?", fact.title),
            vec![fact.address.clone()],
        ),
        Kind::SectionAddress => (
            format!(
                "What is the address of the section \"{}\" on world:{}?",
                fact.title, fact.page
            ),
            vec![fact.address.clone()],
        ),
    };
    Question {
        n,
        kind,
        page: fact.page.clone(),
        text,
        expects,
    }
}

/// An answer's words: lowercase, with quotes, brackets and trailing punctuation off, each with
/// whether it said `world:` and without that prefix.
fn tokens(answer: &str) -> Vec<(String, bool)> {
    answer
        .to_lowercase()
        .split(|c: char| {
            c.is_whitespace() || matches!(c, ',' | ';' | '"' | '\'' | '(' | ')' | '`' | '*')
        })
        .map(|w| w.trim_end_matches(['.', ':', '?', '!']))
        .filter(|w| !w.is_empty())
        .map(|w| match w.strip_prefix("world:") {
            Some(rest) => (rest.to_owned(), true),
            None => (w.to_owned(), false),
        })
        .collect()
}

/// Whether `answer` is a right answer to `question`: for an address, the address said with its
/// `world:` among the words (or the bare address alone, as the whole answer); for who keeps a
/// page, a right name among them. An answer longer than [`ANSWER_MOST`] characters is not one.
#[must_use]
pub fn right(question: &Question, answer: &str) -> bool {
    if answer.chars().count() > ANSWER_MOST {
        return false;
    }
    let said = tokens(answer);
    question.expects.iter().any(|expected| {
        let expected = expected.strip_prefix("world:").unwrap_or(expected);
        if question.kind == Kind::WhoKeeps {
            return said.iter().any(|(w, _)| w == expected);
        }
        said.iter()
            .any(|(w, prefixed)| w == expected && (*prefixed || said.len() == 1))
    })
}

/// How each answer went: one grade a question, a missing answer a wrong one.
#[must_use]
pub fn grade(questions: &[Question], answers: &[String]) -> Vec<Graded> {
    questions
        .iter()
        .map(|q| Graded {
            n: q.n,
            kind: q.kind,
            page: q.page.clone(),
            passed: answers.get(q.n).is_some_and(|a| right(q, a)),
        })
        .collect()
}

/// For each answered quiz in `history` (oldest first) that asked about `page`, whether the page
/// passed that night: at least one question about it right.
#[must_use]
pub fn nights_of(history: &[Quiz], page: &str) -> Vec<bool> {
    history
        .iter()
        .filter(|q| q.status == Status::Answered)
        .filter_map(|q| {
            let about: Vec<&Graded> = q.graded.iter().filter(|g| g.page == page).collect();
            (!about.is_empty()).then(|| about.iter().any(|g| g.passed))
        })
        .collect()
}

/// Whether `page` failed its last [`FAILING_NIGHTS`] nights it was asked about, every one.
#[must_use]
pub fn flagged(history: &[Quiz], page: &str) -> bool {
    let nights = nights_of(history, page);
    nights.len() >= FAILING_NIGHTS
        && nights
            .iter()
            .rev()
            .take(FAILING_NIGHTS)
            .all(|passed| !passed)
}

/// A page's score over the last [`SCORED_OVER`] answered quizzes: questions right and asked.
#[must_use]
pub fn score_of(history: &[Quiz], page: &str) -> (usize, usize) {
    let answered: Vec<&Quiz> = history
        .iter()
        .filter(|q| q.status == Status::Answered)
        .collect();
    let recent = answered.iter().rev().take(SCORED_OVER);
    let mut score = (0, 0);
    for q in recent {
        for g in q.graded.iter().filter(|g| g.page == page) {
            score.1 += 1;
            score.0 += usize::from(g.passed);
        }
    }
    score
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::map::PageKey;
    use crate::domain::situations::Weight;
    use crate::domain::work::{GoalId, Role};

    fn page(key: PageKey, title: &str, topic: &str, steward: &str, answers: &[&str]) -> Fact {
        Fact::page(
            key,
            title,
            topic,
            steward.to_owned(),
            answers.iter().map(|a| (*a).to_owned()).collect(),
            Weight::Core,
        )
    }

    fn facts() -> Vec<Fact> {
        vec![
            page(
                PageKey::Role(Role::Developer),
                "The developer role",
                "how a developer works here",
                "the owner",
                &["owner", "board"],
            ),
            page(
                PageKey::Area(MemberId(2)),
                "Ada's area",
                "how Ada's area works",
                "Ada",
                &["ada"],
            ),
            page(
                PageKey::House,
                "The house",
                "what the team knows about the house",
                "Mara, the CEO",
                &["ceo", "mara"],
            ),
            page(
                PageKey::Lab,
                "How the lab is built",
                "how the lab is built",
                "HQ",
                &["hq", "code"],
            ),
            page(
                PageKey::Goal(GoalId(1)),
                "Goal: Upkeep",
                "why the goal exists",
                "Mara, the CEO",
                &["ceo", "mara"],
            ),
            Fact::section(
                PageKey::Area(MemberId(2)),
                "backups",
                "Backups",
                "Nightly at 03:00",
                "insight",
                Weight::Recommended,
            ),
        ]
    }

    #[test]
    fn a_night_asks_each_kind_in_turn_about_different_pages_and_the_same_night_the_same() {
        let all = facts();
        let asked = questions(&all, 20_744);
        assert_eq!(asked.len(), MOST_QUESTIONS);
        assert_eq!(asked, questions(&all, 20_744), "deterministic");
        let kinds: Vec<Kind> = asked.iter().map(|q| q.kind).collect();
        for kind in [
            Kind::WhichPage,
            Kind::WhoKeeps,
            Kind::AddressOf,
            Kind::SectionAddress,
        ] {
            assert!(kinds.contains(&kind), "{kind:?} in {kinds:?}");
        }
        assert!(asked.iter().enumerate().all(|(i, q)| q.n == i));
        // Every question is about something on the map.
        assert!(asked.iter().all(|q| all.iter().any(|f| f.page == q.page)));
        // The next night starts elsewhere, so the pages asked about move round.
        let pages = |n| {
            questions(&all, n)
                .iter()
                .map(|q| q.page.clone())
                .collect::<Vec<_>>()
        };
        assert_ne!(pages(20_744), pages(20_745));
        let mut seen: Vec<String> = Vec::new();
        for night in 20_744..20_752 {
            seen.extend(pages(night));
        }
        for f in all.iter().filter(|f| !f.section) {
            assert!(
                seen.contains(&f.page),
                "{} is asked about in some night",
                f.page
            );
        }
        assert!(questions(&[], 1).is_empty());
    }

    #[test]
    fn answers_are_scored_by_code_against_the_map() {
        let all = facts();
        let asked = questions(&all, 20_744);
        let right_answers: Vec<String> = asked
            .iter()
            .map(|q| q.expects.first().cloned().unwrap())
            .collect();
        let graded = grade(&asked, &right_answers);
        assert!(graded.iter().all(|g| g.passed), "{graded:?}");
        // A wrong, a missing and a rambling answer all fail.
        let mut mixed = right_answers.clone();
        mixed[0] = "I have no idea".to_owned();
        mixed[1] = "x ".repeat(80);
        mixed.truncate(3);
        let graded = grade(&asked, &mixed);
        assert_eq!(
            graded.iter().map(|g| g.passed).collect::<Vec<_>>(),
            [false, false, true, false, false, false]
        );
    }

    #[test]
    fn an_answer_is_a_right_one_in_the_ways_a_person_writes_it() {
        let q = |kind, expects: &[&str]| Question {
            n: 0,
            kind,
            page: "lab".to_owned(),
            text: String::new(),
            expects: expects.iter().map(|e| (*e).to_owned()).collect(),
        };
        let address = q(Kind::WhichPage, &["world:lab"]);
        for yes in [
            "world:lab",
            "World:Lab.",
            "It is `world:lab`",
            "lab",
            "(world:lab)",
        ] {
            assert!(right(&address, yes), "{yes}");
        }
        for no in ["world:team", "world:labs", "", "the lab page"] {
            assert!(!right(&address, no), "{no}");
        }
        let section = q(Kind::SectionAddress, &["world:area/2#backups"]);
        assert!(right(&section, "world:area/2#backups, I think"));
        assert!(!right(&section, "world:area/2"));
        let who = q(Kind::WhoKeeps, &["ceo", "mara"]);
        assert!(right(&who, "Mara") && right(&who, "The CEO.") && right(&who, "mara, the ceo"));
        assert!(!right(&who, "Ada") && !right(&who, "the owner"));
    }

    #[test]
    fn a_page_is_flagged_when_it_fails_every_one_of_its_last_three_nights() {
        let night = |n: i64, passed: bool, page: &str| Quiz {
            id: n,
            night: n,
            status: Status::Answered,
            member: Some(MemberId(1)),
            asked_at: Some(0),
            answered_at: Some(1),
            questions: Vec::new(),
            answers: Vec::new(),
            graded: vec![Graded {
                n: 0,
                kind: Kind::WhichPage,
                page: page.to_owned(),
                passed,
            }],
            why: String::new(),
        };
        let mut history = vec![
            night(1, true, "lab"),
            night(2, false, "lab"),
            night(3, false, "lab"),
        ];
        assert!(!flagged(&history, "lab"), "two nights is not three");
        history.push(night(4, false, "lab"));
        assert!(flagged(&history, "lab"));
        assert!(!flagged(&history, "team"), "never asked about");
        history.push(night(5, true, "lab"));
        assert!(!flagged(&history, "lab"), "one pass clears it");
        // A night it was not asked about neither fails nor clears it.
        let mut other = vec![
            night(1, false, "house"),
            night(2, false, "house"),
            night(3, false, "lab"),
            night(4, false, "house"),
        ];
        assert!(flagged(&other, "house"));
        other[1].status = Status::Unanswered;
        assert!(
            !flagged(&other, "house"),
            "an unanswered night counts for nothing"
        );
        assert_eq!(score_of(&history, "lab"), (2, 5));
    }

    #[test]
    fn a_quiz_is_kept_and_read_back_whole() {
        let all = facts();
        let asked = questions(&all, 3);
        assert_eq!(Quiz::read_questions(&Quiz::keep_questions(&asked)), asked);
        let answers = vec!["world:lab".to_owned(), "Ada".to_owned()];
        assert_eq!(Quiz::read_answers(&Quiz::keep_answers(&answers)), answers);
        let graded = grade(&asked, &answers);
        assert_eq!(Quiz::read_graded(&Quiz::keep_graded(&graded)), graded);
        assert_eq!(Quiz::read_questions("not json"), Vec::new());
    }
}
