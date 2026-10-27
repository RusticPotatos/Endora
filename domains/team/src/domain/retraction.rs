//! Retracted claims (ADR 0124): when the owner says a note, a section of the world or a story
//! is not true, HQ keeps the claim's fingerprint, finds what else says the same, and refuses it
//! when a member writes it again.
//!
//! A wrong claim spreads the way a right one does: one member notes it, the next reads it and
//! writes it down in its own words. Exact strings are useless against that (model text
//! paraphrases, and an exact comparison of model output is a mistake this project has made),
//! so a claim is compared by its **fingerprint**: its words lowercased with punctuation
//! stripped, taken two at a time (word shingles), and matched by how many of the retracted
//! claim's shingles a text holds.

/// A text's words, as a fingerprint reads them: lowercase, letters and digits only.
#[must_use]
pub fn words(text: &str) -> Vec<String> {
    let cleaned: String = text
        .chars()
        .map(|c| {
            if c.is_alphanumeric() {
                c.to_lowercase().next().unwrap_or(c)
            } else {
                ' '
            }
        })
        .collect();
    cleaned.split_whitespace().map(str::to_owned).collect()
}

/// A text's fingerprint: its words two at a time (one at a time for a one-word text), each
/// once, sorted.
#[must_use]
pub fn fingerprint(text: &str) -> Vec<String> {
    let words = words(text);
    let mut shingles: Vec<String> = if words.len() < 2 {
        words
    } else {
        words.windows(2).map(|w| w.join(" ")).collect()
    };
    shingles.sort();
    shingles.dedup();
    shingles
}

/// How alike a text is to a retracted claim: how many of the claim's shingles it holds, and
/// what share of them that is, in whole percent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Likeness {
    /// The claim's shingles the text holds.
    pub shared: usize,
    /// What share of the claim's shingles that is.
    pub percent: usize,
}

/// How alike `text` (a fingerprint) is to `claim` (one).
#[must_use]
pub fn likeness(claim: &[String], text: &[String]) -> Likeness {
    if claim.is_empty() {
        return Likeness {
            shared: 0,
            percent: 0,
        };
    }
    let shared = claim.iter().filter(|s| text.contains(s)).count();
    Likeness {
        shared,
        percent: shared * 100 / claim.len(),
    }
}

/// A fingerprint as it is kept: its shingles, `|` between.
#[must_use]
pub fn kept(fingerprint: &[String]) -> String {
    fingerprint.join("|")
}

/// A fingerprint read back from how it is kept.
#[must_use]
pub fn read(kept: &str) -> Vec<String> {
    kept.split('|')
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fingerprint_is_its_words_two_at_a_time_lowercase_without_punctuation() {
        assert_eq!(
            fingerprint("Sonarr needs a restart, every night!"),
            [
                "a restart",
                "every night",
                "needs a",
                "restart every",
                "sonarr needs"
            ]
        );
        assert_eq!(fingerprint("Deluge."), ["deluge"]);
        assert_eq!(fingerprint("  "), Vec::<String>::new());
        assert_eq!(read(&kept(&fingerprint("a b c"))), fingerprint("a b c"));
    }
}
