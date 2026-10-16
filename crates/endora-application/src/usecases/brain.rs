//! The brain's use cases (ADR 0101): reading a page for the butler, and noting what it
//! learned onto the page it belongs on.

use super::*;
use crate::ports::UntrustedReader;
use endora_understanding::PagePath;
use endora_understanding::domain::brain;

/// Where a fact goes when the model named no page, or one Endora may not write to: memory is
/// what the person told it and what it folds there itself, never a place for a raw note.
const UNSORTED: &str = "unsorted";

/// The person's own date, `YYYY-MM-DD`, for a timeline line.
pub fn today(clock: &(impl Clock + ?Sized)) -> String {
    let now = clock.now().unix_millis();
    brain::date_of(now + clock.utc_offset_minutes() * 60_000)
}

/// Notes what the butler learned onto the brain's pages, one timeline line each, and says
/// so in `activity`. The same rules as ever decide what is worth keeping: a command they
/// gave, something about Endora's own conduct, anything tied to a day, and a passing remark
/// or a one-off ask (kind `intent` or `other`) are not facts about the person. The same fact
/// twice is one line.
///
/// # Errors
/// [`AppError::Repository`] if the brain cannot be written.
pub fn note_what_it_learned(
    brain: &impl Brain,
    formed: Vec<FormedBelief>,
    today: &str,
    activity: &mut Vec<String>,
) -> Result<(), AppError> {
    for proposed in formed {
        if reads_as_an_instruction(&proposed.statement)
            || is_about_endora_itself(&proposed.evidence)
            || pinned_to_a_day(&proposed.statement)
            || matches!(
                proposed.kind,
                crate::BeliefKind::Intent | crate::BeliefKind::Other
            )
        {
            continue;
        }
        let Ok(fact) = brain::one_fact(&proposed.statement) else {
            continue;
        };
        let page = PagePath::parse(&proposed.page)
            .ok()
            .filter(|page| !page.is_memory())
            .unwrap_or_else(|| PagePath::parse(UNSORTED).expect("a page name"));
        let changed = brain.change(&page, today, "noted from chat", &mut |text| match text {
            Some(text) => brain::with_timeline_entry(text, today, &fact),
            None => Some(brain::new_page(&page.title_guess(), today, &fact)),
        })?;
        if changed {
            activity.push(format!("Noted in {page}: {fact}"));
        }
    }
    Ok(())
}

/// The most of one page a read hands the butler. Pages are notes; one grown past this by
/// hand is cut, with a note, rather than flooding the turn.
const PAGE_IN_A_TURN: usize = 6_000;

/// A page's text for the butler, or, when there is no such page, what pages there are, so a
/// wrong guess at a name is one step from the right one.
///
/// # Errors
/// [`AppError::BadRequest`] for a name that is not a page name;
/// [`AppError::Repository`] if the brain cannot be read.
pub fn read_page(brain: &impl Brain, name: &str) -> Result<String, AppError> {
    let path = PagePath::parse(name).map_err(|e| AppError::BadRequest {
        message: e.to_string(),
    })?;
    if let Some(text) = brain.read(&path)? {
        if text.chars().count() <= PAGE_IN_A_TURN {
            return Ok(text);
        }
        let cut: String = text.chars().take(PAGE_IN_A_TURN).collect();
        return Ok(format!(
            "{cut}\n… (the page goes on; this is as much as a turn carries)"
        ));
    }
    let names: Vec<String> = brain
        .pages()?
        .into_iter()
        .map(|page| page.path.as_str().to_owned())
        .collect();
    if names.is_empty() {
        return Ok(format!("There is no page called {path}, and no pages yet."));
    }
    Ok(format!(
        "There is no page called {path}. The pages are: {}.",
        names.join(", ")
    ))
}

/// The largest page the person can save from the console: the size the brain will read.
const LARGEST_EDIT: usize = 256 * 1024;

/// Saves a page as the person wrote it in the console, as their change in its history.
/// Returns whether anything changed.
///
/// # Errors
/// [`AppError::BadRequest`] for a name that is not a page name or a page too large to read;
/// [`AppError::Repository`] if the brain cannot be written.
pub fn edit_page(brain: &impl Brain, name: &str, text: &str, when: &str) -> Result<bool, AppError> {
    let path = PagePath::parse(name).map_err(|e| AppError::BadRequest {
        message: e.to_string(),
    })?;
    if text.len() > LARGEST_EDIT {
        return Err(AppError::BadRequest {
            message: "a page is notes, and this is larger than a page the brain reads".to_owned(),
        });
    }
    let mut text = text.replace("\r\n", "\n");
    if !text.ends_with('\n') {
        text.push('\n');
    }
    Ok(brain.change(&path, when, "edited by you", &mut |_| Some(text.clone()))?)
}

/// Removes a page; its history keeps it. Memory is emptied by editing, never removed.
///
/// # Errors
/// [`AppError::BadRequest`] for a name that is not a page or for memory;
/// [`AppError::Repository`] if the brain cannot be written.
pub fn delete_page(brain: &impl Brain, name: &str, when: &str) -> Result<bool, AppError> {
    let path = PagePath::parse(name).map_err(|e| AppError::BadRequest {
        message: e.to_string(),
    })?;
    if path.is_memory() {
        return Err(AppError::BadRequest {
            message: "memory is not removed; edit it to empty it".to_owned(),
        });
    }
    Ok(brain.delete(&path, when, "deleted by you")?)
}

/// Forgets everything Endora learned, history and all, and keeps what the person told it:
/// the lines of memory's "What you told me" go into the new, empty brain. Returns how many
/// pages went, memory not counted.
///
/// # Errors
/// [`AppError::Repository`] if the brain cannot be read, emptied or written.
pub fn forget_what_it_learned(brain: &impl Brain, when: &str) -> Result<usize, AppError> {
    let told = brain
        .read(&PagePath::memory())?
        .map(|text| brain::section(&text, brain::TOLD))
        .unwrap_or_default();
    let pages = brain
        .pages()?
        .iter()
        .filter(|page| !page.path.is_memory())
        .count();
    brain.forget_everything()?;
    if !told.is_empty() {
        brain.change(
            &PagePath::memory(),
            when,
            "kept what you told me",
            &mut |text| {
                let text = text.map_or_else(brain::new_memory, str::to_owned);
                Some(brain::with_section(&text, brain::TOLD, &told))
            },
        )?;
    }
    Ok(pages)
}

/// Why a change was a note from chat: the one kind of change a fold follows.
const NOTED: &str = "noted from chat";
/// Why a change was the person's own edit in the console.
const THEIRS: &str = "edited by you";
/// The longest summary a fold may write. A summary is a few sentences; anything longer
/// was not a summary.
const LONGEST_SUMMARY: usize = 1_200;

const FOLD_INSTRUCTIONS: &str = "Below is one page of notes about a person: a title, a \
summary, and a dated timeline of facts. Write the summary again so it says what is true now, \
in two to five plain sentences that address them as 'you'. A later line wins over an earlier \
one. Use only what the page says; add nothing. Reply with the summary alone: no title, no \
heading, no list, no preamble.";

const KNOWS_INSTRUCTIONS: &str = "Below is a list of pages of notes about a person, one line \
each. Pick the few things that matter most for helping them day to day and write each as one \
short line that addresses them as 'you'. At most twelve lines. Use only what the list says. \
Reply with the lines alone, one per line, no bullets, no numbering, no preamble.";

/// Folds one page whose timeline has grown since its summary was written: the local model
/// writes the summary again from the page, and nothing else on the page moves. A page whose
/// latest change was the person's own is left alone. After a fold, memory's "What I know
/// best" is written again from the page list, unless the person has made it theirs (their
/// edit was memory's latest change and the section is not empty). Returns the page folded,
/// if any; one page at a time, so a quiet hour is never one long hold on the model.
///
/// # Errors
/// [`AppError::Repository`] if the brain cannot be read or written. A model that cannot be
/// reached, or that answers with something that is not a summary, folds nothing.
pub fn fold_one_page(
    brain: &impl Brain,
    reader: &dyn UntrustedReader,
    today: &str,
) -> Result<Option<PagePath>, AppError> {
    let mut due = None;
    for page in brain.pages()? {
        if page.path.is_memory() {
            continue;
        }
        let last = brain.history(&page.path)?;
        if last.first().is_some_and(|change| change.why == NOTED) {
            due = Some(page.path);
            break;
        }
    }
    let Some(path) = due else {
        return Ok(None);
    };
    let Some(text) = brain.read(&path)? else {
        return Ok(None);
    };
    let Ok(written) = reader.read(FOLD_INSTRUCTIONS, &text) else {
        return Ok(None);
    };
    let Some(summary) = a_summary(&written) else {
        return Ok(None);
    };
    brain.change(&path, today, "summary folded", &mut |text| {
        text.map(|text| brain::with_summary(text, &path, &summary))
    })?;
    refresh_what_it_knows_best(brain, reader, today)?;
    Ok(Some(path))
}

/// What the model wrote, if it is a summary: some words, not too many, and no structure that
/// would rewrite the page around it.
fn a_summary(written: &str) -> Option<String> {
    let lines: Vec<&str> = written
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    if lines.is_empty() || lines.iter().any(|line| line.starts_with('#')) {
        return None;
    }
    let summary = lines.join(" ");
    (summary.chars().count() <= LONGEST_SUMMARY).then_some(summary)
}

fn refresh_what_it_knows_best(
    brain: &impl Brain,
    reader: &dyn UntrustedReader,
    today: &str,
) -> Result<(), AppError> {
    let memory = PagePath::memory();
    let text = brain.read(&memory)?.unwrap_or_else(brain::new_memory);
    let theirs = brain
        .history(&memory)?
        .first()
        .is_some_and(|change| change.why == THEIRS);
    if theirs && !brain::section(&text, brain::KNOWS).is_empty() {
        return Ok(());
    }
    let pages: Vec<String> = brain
        .pages()?
        .into_iter()
        .filter(|page| !page.path.is_memory())
        .map(|page| format!("{}: {}", page.title, page.line))
        .collect();
    if pages.is_empty() {
        return Ok(());
    }
    let Ok(written) = reader.read(KNOWS_INSTRUCTIONS, &pages.join("\n")) else {
        return Ok(());
    };
    let lines: Vec<String> = written
        .lines()
        .map(|line| line.trim().trim_start_matches(['-', '*', '•']).trim())
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| brain::one_fact(line).ok())
        .take(brain::MOST_MEMORY_LINES)
        .collect();
    if lines.is_empty() {
        return Ok(());
    }
    brain.change(
        &memory,
        today,
        "what it knows best, refreshed",
        &mut |text| {
            let text = text.map_or_else(brain::new_memory, str::to_owned);
            Some(brain::with_section(&text, brain::KNOWS, &lines))
        },
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use endora_understanding::BrainFolder;

    #[test]
    fn a_page_is_read_by_the_name_people_write() {
        let brain = BrainFolder::temporary().unwrap();
        let watches = PagePath::parse("hobbies/watches").unwrap();
        brain
            .change(&watches, "2026-10-16", "test", &mut |_| {
                Some("# Watches\n\nCollects dive watches.\n".to_owned())
            })
            .unwrap();
        assert!(
            read_page(&brain, "Hobbies/Watches")
                .unwrap()
                .contains("Collects dive watches.")
        );
    }

    fn proposed(statement: &str, kind: crate::BeliefKind, page: &str) -> FormedBelief {
        FormedBelief {
            statement: statement.to_owned(),
            kind,
            confidence: crate::Confidence::Medium,
            evidence: "they said so".to_owned(),
            page: page.to_owned(),
        }
    }

    #[test]
    fn what_it_learned_goes_on_its_page_once_and_says_so() {
        use crate::BeliefKind::{Pattern, Preference};
        let brain = BrainFolder::temporary().unwrap();
        let mut activity = Vec::new();
        note_what_it_learned(
            &brain,
            vec![
                proposed("You collect dive watches", Pattern, "Hobbies/Watches"),
                proposed("you collect dive watches.", Pattern, "hobbies/watches"),
                proposed("You like short answers", Preference, "style"),
            ],
            "2026-10-16",
            &mut activity,
        )
        .unwrap();
        let watches = brain
            .read(&PagePath::parse("hobbies/watches").unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(
            brain::timeline_of(&watches),
            ["2026-10-16: You collect dive watches"]
        );
        assert_eq!(
            activity,
            [
                "Noted in hobbies/watches: You collect dive watches",
                "Noted in style: You like short answers"
            ]
        );
    }

    #[test]
    fn what_is_not_a_lasting_fact_is_not_noted() {
        use crate::BeliefKind::{Intent, Other, Preference};
        let brain = BrainFolder::temporary().unwrap();
        let mut activity = Vec::new();
        note_what_it_learned(
            &brain,
            vec![
                proposed(
                    "You want me to turn off the kitchen light",
                    Preference,
                    "home",
                ),
                proposed("You have an appointment tomorrow", Preference, "routine"),
                proposed("You want to know the score", Intent, "sport"),
                proposed("You mentioned the weather", Other, "misc"),
            ],
            "2026-10-16",
            &mut activity,
        )
        .unwrap();
        assert!(activity.is_empty(), "{activity:?}");
        assert_eq!(brain.pages().unwrap().len(), 1, "only memory");
    }

    #[test]
    fn a_note_aimed_at_memory_or_nowhere_goes_to_unsorted() {
        use crate::BeliefKind::Value;
        let brain = BrainFolder::temporary().unwrap();
        let mut activity = Vec::new();
        note_what_it_learned(
            &brain,
            vec![
                proposed("You value quiet evenings", Value, "memory"),
                proposed("You value your family", Value, "../../etc"),
            ],
            "2026-10-16",
            &mut activity,
        )
        .unwrap();
        let unsorted = brain
            .read(&PagePath::parse("unsorted").unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(brain::timeline_of(&unsorted).len(), 2);
        let memory = brain.read(&PagePath::memory()).unwrap().unwrap();
        assert!(
            !memory.contains("quiet evenings"),
            "memory is not written by a note"
        );
    }

    #[test]
    fn forgetting_keeps_what_they_told_it_and_nothing_else_not_even_history() {
        let brain = BrainFolder::temporary().unwrap();
        brain
            .change(&PagePath::memory(), "", "test", &mut |text| {
                let text = text.unwrap().to_owned();
                let text =
                    brain::with_section(&text, brain::TOLD, &["Based in: New York".to_owned()]);
                Some(brain::with_section(
                    &text,
                    brain::KNOWS,
                    &["Busy weekdays".to_owned()],
                ))
            })
            .unwrap();
        let mut activity = Vec::new();
        note_what_it_learned(
            &brain,
            vec![proposed(
                "You collect dive watches",
                crate::BeliefKind::Pattern,
                "hobbies/watches",
            )],
            "2026-10-16",
            &mut activity,
        )
        .unwrap();
        assert_eq!(forget_what_it_learned(&brain, "2026-10-16").unwrap(), 1);
        let memory = brain.read(&PagePath::memory()).unwrap().unwrap();
        assert_eq!(brain::section(&memory, brain::TOLD), ["Based in: New York"]);
        assert!(brain::section(&memory, brain::KNOWS).is_empty());
        assert_eq!(brain.pages().unwrap().len(), 1, "only memory");
        let watches = PagePath::parse("hobbies/watches").unwrap();
        assert!(brain.history(&watches).unwrap().is_empty());
    }

    #[test]
    fn an_edit_is_theirs_and_memory_is_never_deleted() {
        let brain = BrainFolder::temporary().unwrap();
        assert!(edit_page(&brain, "Routine", "# Routine\r\n\nUp at six.", "2026-10-16").unwrap());
        let routine = PagePath::parse("routine").unwrap();
        assert_eq!(
            brain.read(&routine).unwrap().unwrap(),
            "# Routine\n\nUp at six.\n"
        );
        assert_eq!(brain.history(&routine).unwrap()[0].why, "edited by you");
        assert!(delete_page(&brain, "memory", "").is_err());
        assert!(delete_page(&brain, "routine", "2026-10-16").unwrap());
        assert!(edit_page(&brain, "../x", "y", "").is_err());
    }

    /// The local model, as far as folding needs it: a summary for a page, lines for memory.
    struct Writes {
        summary: &'static str,
        knows: &'static str,
        asked: std::cell::RefCell<Vec<String>>,
    }
    impl crate::UntrustedReader for Writes {
        fn read(&self, instructions: &str, data: &str) -> Result<String, crate::ProposalError> {
            self.asked.borrow_mut().push(data.to_owned());
            if instructions == FOLD_INSTRUCTIONS {
                return Ok(self.summary.to_owned());
            }
            Ok(self.knows.to_owned())
        }
    }

    fn writes(summary: &'static str, knows: &'static str) -> Writes {
        Writes {
            summary,
            knows,
            asked: std::cell::RefCell::new(Vec::new()),
        }
    }

    fn noted(brain: &BrainFolder, page: &str, fact: &str) {
        let mut activity = Vec::new();
        note_what_it_learned(
            brain,
            vec![proposed(fact, crate::BeliefKind::Pattern, page)],
            "2026-10-16",
            &mut activity,
        )
        .unwrap();
    }

    #[test]
    fn a_page_that_grew_is_folded_and_what_it_knows_best_follows() {
        let brain = BrainFolder::temporary().unwrap();
        noted(&brain, "hobbies/watches", "You collect dive watches");
        noted(&brain, "hobbies/watches", "You want a GMT next");
        let model = writes(
            "You collect dive watches and want a GMT next.",
            "- You collect dive watches\n- You are busy Monday to Friday",
        );
        let folded = fold_one_page(&brain, &model, "2026-10-17").unwrap();
        assert_eq!(folded.unwrap().as_str(), "hobbies/watches");
        let page = brain
            .read(&PagePath::parse("hobbies/watches").unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(
            brain::summary_of(&page),
            "You collect dive watches and want a GMT next."
        );
        assert_eq!(
            brain::timeline_of(&page).len(),
            2,
            "the timeline is untouched"
        );
        let memory = brain.read(&PagePath::memory()).unwrap().unwrap();
        assert_eq!(
            brain::section(&memory, brain::KNOWS),
            ["You collect dive watches", "You are busy Monday to Friday"]
        );
        // Folded, it is not due again until something new is noted.
        assert_eq!(fold_one_page(&brain, &model, "2026-10-17").unwrap(), None);
    }

    #[test]
    fn a_page_they_edited_and_a_summary_that_is_not_one_are_left_alone() {
        let brain = BrainFolder::temporary().unwrap();
        noted(&brain, "routine", "You are up at six");
        edit_page(&brain, "routine", "# Routine\n\nMine.\n", "2026-10-16").unwrap();
        let model = writes("Something else.", "You are up at six");
        assert_eq!(fold_one_page(&brain, &model, "2026-10-17").unwrap(), None);
        assert!(
            model.asked.borrow().is_empty(),
            "the model was not even asked"
        );

        noted(&brain, "hobbies/tech", "You like small computers");
        let model = writes("## A heading\nthat would rewrite the page", "x");
        assert_eq!(fold_one_page(&brain, &model, "2026-10-17").unwrap(), None);
        let page = brain
            .read(&PagePath::parse("hobbies/tech").unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(brain::summary_of(&page), "You like small computers");
    }

    #[test]
    fn what_it_knows_best_is_theirs_once_they_have_written_it() {
        let brain = BrainFolder::temporary().unwrap();
        let mut memory = brain.read(&PagePath::memory()).unwrap().unwrap();
        memory = brain::with_section(&memory, brain::KNOWS, &["My own line".to_owned()]);
        edit_page(&brain, "memory", &memory, "2026-10-16").unwrap();
        noted(&brain, "routine", "You are up at six");
        let model = writes("You are up at six.", "Something the model chose");
        assert!(
            fold_one_page(&brain, &model, "2026-10-17")
                .unwrap()
                .is_some()
        );
        let memory = brain.read(&PagePath::memory()).unwrap().unwrap();
        assert_eq!(brain::section(&memory, brain::KNOWS), ["My own line"]);
    }

    #[test]
    fn a_wrong_name_gets_the_list_and_a_bad_one_is_refused() {
        let brain = BrainFolder::temporary().unwrap();
        let said = read_page(&brain, "hobbies/watch").unwrap();
        assert!(said.contains("no page called hobbies/watch"), "{said}");
        assert!(said.contains("memory"), "{said}");
        assert!(read_page(&brain, "../etc/passwd").is_err());
    }
}
