//! Standing in for the person before anything leaves the house (ADR 0051).
//!
//! The deep model is somebody else's. Sending it a brief means sending a name, a city, and
//! the title of an appointment — and the guard that claimed to prevent this replaced email
//! addresses and nothing else.
//!
//! **Endora does not have to detect personal information, because it already knows it.**
//! The person's name is in their preferences, their city is in their preferences, the event
//! title came from their own calendar. Guessing at PII with patterns is what you do when you
//! lack the values; substituting values you hold is exact, and it fails towards over-hiding
//! rather than under-hiding.
//!
//! What goes out is a placeholder; what comes back has the real thing put back locally, so
//! the person reads their own words and the remote model never saw them:
//!
//! ```text
//! out:   on the Family calendar: <event 1> at 18:30; <person 1> is not home
//! back:  Good morning. <person 1> isn't home, and <event 1> is at 6:30.
//! shown: Good morning. morgan isn't home, and K. Novak & J. Ellis is at 6:30.
//! ```
//!
//! ## What this does not do
//!
//! It hides **values, not structure**. That an appointment exists at 18:30, and that the
//! house is empty, still leave — and no substitution can prevent that while the model is
//! being asked to write about them. Anyone worried about the *shape* of their day reaching a
//! third party should not send it at all, and the honest way to offer that is a switch, not
//! a cleverer disguise.

use std::collections::BTreeMap;

/// A two-way table between what is real and what is sent.
#[derive(Debug, Default, Clone)]
pub struct Pseudonyms {
    /// `(real, placeholder)`, longest real value first.
    standing_in: Vec<(String, String)>,
}

impl Pseudonyms {
    /// Builds a table from values Endora holds, each labelled by what kind of thing it is.
    ///
    /// `kinds` maps a label (`"person"`, `"place"`, `"event"`) to the real values of that
    /// kind. Very short values are skipped: a two-character "name" would match inside
    /// ordinary words and turn the request into confetti.
    #[must_use]
    pub fn of(kinds: &BTreeMap<&str, Vec<String>>) -> Self {
        /// Short enough to appear inside unrelated words.
        const TOO_SHORT_TO_SUBSTITUTE: usize = 3;
        let mut standing_in: Vec<(String, String)> = Vec::new();
        for (kind, values) in kinds {
            let mut seen = 0;
            for real in values {
                let real = real.trim();
                if real.len() <= TOO_SHORT_TO_SUBSTITUTE
                    || standing_in.iter().any(|(had, _)| had == real)
                {
                    continue;
                }
                seen += 1;
                standing_in.push((real.to_owned(), format!("<{kind} {seen}>")));
            }
        }
        // Longest first: "K. Novak & J. Ellis" must go before "Jordan Ellis", or
        // a shorter match leaves half a name behind in the outgoing text.
        standing_in.sort_by_key(|(real, _)| std::cmp::Reverse(real.len()));
        Self { standing_in }
    }

    /// Whether there is anything to hide.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.standing_in.is_empty()
    }

    /// The text as it may leave: real values swapped for placeholders.
    #[must_use]
    pub fn hide(&self, text: &str) -> String {
        self.standing_in
            .iter()
            .fold(text.to_owned(), |carried, (real, stands_in)| {
                carried.replace(real, stands_in)
            })
    }

    /// The text as the person reads it: placeholders swapped back.
    ///
    /// A placeholder the model reworded is simply not replaced, and shows as itself. That is
    /// the safe direction to fail: an odd-looking `<person 1>` on screen is a nuisance, where
    /// the reverse would be the leak this exists to prevent.
    #[must_use]
    pub fn restore(&self, text: &str) -> String {
        self.standing_in
            .iter()
            .fold(text.to_owned(), |carried, (real, stands_in)| {
                carried.replace(stands_in, real)
            })
    }
}

#[cfg(test)]
mod tests {
    use super::{BTreeMap, Pseudonyms};

    fn table() -> Pseudonyms {
        let mut kinds: BTreeMap<&str, Vec<String>> = BTreeMap::new();
        kinds.insert(
            "person",
            vec!["morgan".to_owned(), "Jordan Ellis".to_owned()],
        );
        kinds.insert("place", vec!["New York NC".to_owned()]);
        kinds.insert("event", vec!["K. Novak & J. Ellis".to_owned()]);
        Pseudonyms::of(&kinds)
    }

    #[test]
    fn nothing_personal_is_in_what_leaves() {
        // The real brief, as it was actually assembled.
        let brief = "morgan is not home; on the Family calendar: K. Novak & J. Ellis \
                     at 2026-07-31 18:30:00; outside it is 69F";
        let sent = table().hide(brief);

        for real in ["morgan", "K. Novak", "Ellis", "New York"] {
            assert!(!sent.contains(real), "{real} left the house: {sent}");
        }
        // What is not about the person is untouched, or the remote model has nothing to
        // write from.
        assert!(sent.contains("18:30"), "{sent}");
        assert!(sent.contains("69F"), "{sent}");
    }

    #[test]
    fn what_comes_back_is_the_persons_own_words_again() {
        let table = table();
        let sent = table.hide("morgan is not home; K. Novak & J. Ellis at 18:30");
        let answered = format!("Good morning. {sent} — shall I set a reminder?");
        let shown = table.restore(&answered);
        assert!(shown.contains("morgan is not home"), "{shown}");
        assert!(shown.contains("K. Novak & J. Ellis"), "{shown}");
        assert!(!shown.contains("<person"), "{shown}");
    }

    #[test]
    fn a_longer_name_is_replaced_before_the_shorter_one_inside_it() {
        // "Jordan Ellis" and "K. Novak & J. Ellis" share a surname. Replacing the
        // shorter first would leave half a name in the outgoing text — the leak in disguise.
        let sent = table().hide("Jordan Ellis and K. Novak & J. Ellis");
        assert!(!sent.contains("Ellis"), "{sent}");
    }

    #[test]
    fn a_reworded_placeholder_fails_visibly_rather_than_silently() {
        // If the model does not echo a placeholder exactly, the substitution does not
        // happen — an odd token on screen, never the real value going out.
        let table = table();
        let answered = "Good morning, <person one>. You have <the event> tonight.";
        assert_eq!(table.restore(answered), answered);
    }

    #[test]
    fn a_value_too_short_to_be_safe_is_left_alone() {
        // A two-letter "name" would match inside ordinary words and shred the request.
        let mut kinds: BTreeMap<&str, Vec<String>> = BTreeMap::new();
        kinds.insert("person", vec!["Al".to_owned(), "sir".to_owned()]);
        let table = Pseudonyms::of(&kinds);
        assert!(
            table.is_empty(),
            "short values must not become substitutions"
        );
        assert_eq!(table.hide("Already asleep, sir"), "Already asleep, sir");
    }
}
